#![no_std]

pub mod library;
pub use library::*;

pub mod models;
pub use models::*;

elrond_wasm::imports!();
elrond_wasm::derive_imports!();

const STABLE_COIN_NAME: &[u8] = b"StableCoin";
const STABLE_COIN_TICKER: &[u8] = b"STCOIN";

const DEBT_TOKEN_NAME: &[u8] = b"DebtBearing";
const DEBT_TOKEN_TICKER: &[u8] = b"DEBT";

#[elrond_wasm_derive::contract(LiquidityPoolImpl)]
pub trait LiquidityPool {
    #[module(LibraryModuleImpl)]
    fn library_module(&self) -> LibraryModuleImpl<T, BigInt, BigUint>;

    #[init]
    fn init(
        &self,
        asset: TokenIdentifier,
        r_base: BigUint,
        r_slope1: BigUint,
        r_slope2: BigUint,
        u_optimal: BigUint,
        reserve_factor: BigUint,
    ) {
        self.library_module().init();
        self.pool_asset_id().set(&asset);
        self.debt_nonce().set(&1u64);
        self.reserve_data().set(&ReserveData {
            r_base,
            r_slope1,
            r_slope2,
            u_optimal,
            reserve_factor,
        });
    }

    #[payable("*")]
    #[endpoint]
    fn borrow(
        &self,
        #[payment_token] collateral_id: TokenIdentifier,
        #[payment] amount: BigUint,
    ) -> SCResult<()> {
        require!(amount > 0, "amount must be bigger then 0");
        
        let caller = self.get_caller();
        let borrow_token = self.get_borrow_token();
        let asset = self.get_pool_asset();

        let mut borrows_reserve = self
            .reserves()
            .get(&borrow_token)
            .unwrap_or_else(BigUint::zero);
        let mut asset_reserve = self.reserves().get(&asset).unwrap_or_else(BigUint::zero);

        require!(asset_reserve != BigUint::zero(), "asset reserve is empty");

        let position_id = self.get_nft_hash();
        let debt_metadata = DebtMetadata {
            timestamp: self.get_block_timestamp(),
            collateral_amount: amount.clone(),
            collateral_identifier: collateral_id.clone(),
            collateral_timestamp: timestamp,
        };

        self.mint_debt(amount.clone(), debt_metadata.clone(), position_id.clone());

        let nonce = self
            .get_current_esdt_nft_nonce(&self.get_sc_address(), borrow_token.as_esdt_identifier());

        // send debt position tokens

        self.send().direct_esdt_nft_via_transfer_exec(
            &caller,
            &borrow_token.as_esdt_identifier(),
            nonce,
            &amount,
            &[],
        );

        // send collateral requested to the user

        self.send().direct(&caller, &asset, &amount, &[]);

        borrows_reserve += &amount;
        asset_reserve -= &amount;

        self.total_borrow().update(|total| *total += &amount);

        self.reserves().insert(borrow_token, borrows_reserve);
        self.reserves().insert(asset, asset_reserve);

        let current_health = self.compute_health_factor();
        let debt_position = DebtPosition::<BigUint> {
            size: amount.clone(), // this will be initial L tokens amount
            health_factor: current_health,
            is_liquidated: false,
            timestamp: debt_metadata.timestamp,
            collateral_amount: amount,
            collateral_identifier: collateral_id,
        };
        self.debt_positions().insert(position_id, debt_position);

        Ok(())
    }

    #[payable("*")]
    #[endpoint(lockBTokens)]
    fn lock_b_tokens(
        &self,
        initial_caller: Address,
        #[payment_token] borrow_token: TokenIdentifier,
        #[payment] amount: BigUint,
    ) -> SCResult<H256> {
        require!(
            self.get_caller() == self.get_lending_pool(),
            "can only be called by lending pool"
        );
        require!(amount > 0, "amount must be greater then 0");
        require!(!initial_caller.is_zero(), "invalid address");

        require!(
            borrow_token == self.get_borrow_token(),
            "borrow token not supported by this pool"
        );

        let nft_nonce = self.call_value().esdt_token_nonce();

        let esdt_nft_data = self.get_esdt_token_data(
            &self.get_sc_address(),
            borrow_token.as_esdt_identifier(),
            nft_nonce,
        );

        let debt_position_id = esdt_nft_data.hash;
        let debt_position: DebtPosition<BigUint> = self
            .debt_positions()
            .get(&debt_position_id)
            .unwrap_or_default();

        require!(
            debt_position != DebtPosition::default(),
            "invalid debt position"
        );
        require!(!debt_position.is_liquidated, "position is liquidated");

        let metadata: DebtMetadata<BigUint>;
        match DebtMetadata::<BigUint>::top_decode(esdt_nft_data.attributes.as_slice()) {
            Result::Ok(decoded) => {
                metadata = decoded;
            }
            Result::Err(_) => {
                return sc_error!("could not parse token metadata");
            }
        }
        let data = [
            borrow_token.as_esdt_identifier(),
            amount.to_bytes_be().as_slice(),
            &nft_nonce.to_be_bytes()[..],
        ]
        .concat();

        let unique_repay_id = self.keccak256(&data);
        let repay_position = RepayPostion {
            identifier: borrow_token,
            amount,
            nonce: nft_nonce,
            borrow_timestamp: metadata.timestamp,
            collateral_identifier: metadata.collateral_identifier,
            collateral_amount: metadata.collateral_amount,
            collateral_timestamp: metadata.collateral_timestamp,
        };
        self.repay_position()
            .insert(unique_repay_id.clone(), repay_position);

        Ok(unique_repay_id)
    }

    #[payable("*")]
    #[endpoint(repay)]
    fn repay(
        &self,
        unique_id: H256,
        #[payment_token] asset: TokenIdentifier,
        #[payment] amount: BigUint,
    ) -> SCResult<RepayPostion<BigUint>> {
        require!(
            self.get_caller() == self.get_lending_pool(),
            "function can only be called by lending pool"
        );
        require!(amount > 0, "amount must be greater then 0");
        require!(
            asset == self.get_pool_asset(),
            "asset is not supported by this pool"
        );

        require!(
            self.repay_position().contains_key(&unique_id),
            "there are no locked borrowed token for this id, lock b tokens first"
        );
        let mut repay_position = self.repay_position().get(&unique_id).unwrap_or_default();

        require!(
            repay_position.amount >= amount,
            "b tokens amount locked must be equal with the amount of asset token send"
        );

        let esdt_nft_data = self.get_esdt_token_data(
            &self.get_sc_address(),
            repay_position.identifier.as_esdt_identifier(),
            repay_position.nonce,
        );

        let debt_position_id = esdt_nft_data.hash;

        require!(
            self.debt_positions().contains_key(&debt_position_id),
            "invalid debt position id"
        );
        let debt_position = self
            .debt_positions()
            .get(&debt_position_id)
            .unwrap_or_default();

        require!(!debt_position.is_liquidated, "position is liquidated");

        let interest = self.get_debt_interest(
            repay_position.amount.clone(),
            repay_position.borrow_timestamp,
        );

        if repay_position.amount.clone() + interest == amount {
            self.repay_position().remove(&unique_id);
        } else if repay_position.amount > amount {
            repay_position.amount -= amount.clone();
            self.repay_position()
                .insert(unique_id, repay_position.clone());
        }

        self.burn(
            amount.clone(),
            repay_position.nonce,
            repay_position.identifier.clone(),
        );

        repay_position.amount = amount;

        Ok(repay_position)
    }

    #[payable("*")]
    #[endpoint]
    fn withdraw(
        &self,
        initial_caller: Address,
        #[payment_token] lend_token: TokenIdentifier,
        #[payment] amount: BigUint,
    ) -> SCResult<()> {
        require!(
            self.get_caller() == self.get_lending_pool(),
            "liquidity pool can only be called by lending pool"
        );
        require!(
            lend_token == self.get_lend_token(),
            "lend token is not supported by this pool"
        );
        require!(!initial_caller.is_zero(), "invalid address");
        require!(amount > 0, "amount must be bigger then 0");

        let pool_asset = self.get_pool_asset();
        let mut asset_reserve = self
            .reserves()
            .get(&pool_asset)
            .unwrap_or_else(BigUint::zero);

        require!(asset_reserve != BigUint::zero(), "asset reserve is empty");

        let nonce = self.call_value().esdt_token_nonce();
        self.burn(amount.clone(), nonce, lend_token);

        self.send()
            .direct(&initial_caller, &pool_asset, &amount, &[]);

        asset_reserve -= amount;
        self.reserves().insert(pool_asset, asset_reserve);

        Ok(())
    }

    #[payable("*")]
    #[endpoint(liquidate)]
    fn liquidate(
        &self,
        position_id: H256,
        #[payment_token] token: TokenIdentifier,
        #[payment] amount: BigUint,
    ) -> SCResult<LiquidateData<BigUint>> {
        require!(
            self.get_caller() == self.get_lending_pool(),
            "function can only be called by lending pool"
        );
        require!(amount > 0, "amount must be bigger then 0");
        require!(
            token == self.get_pool_asset(),
            "asset is not supported by this pool"
        );

        let mut debt_position = self.debt_positions().get(&position_id).unwrap_or_default();

        require!(
            debt_position != DebtPosition::default(),
            "invalid debt position id"
        );
        require!(
            !debt_position.is_liquidated,
            "position is already liquidated"
        );
        require!(
            debt_position.health_factor < self.get_health_factor_threshold(),
            "the health factor is not low enough"
        );

        let interest = self.get_debt_interest(debt_position.size.clone(), debt_position.timestamp);

        require!(
            debt_position.size.clone() + interest == amount,
            "position can't be liquidated, not enough or to much tokens send"
        );

        debt_position.is_liquidated = true;

        self.debt_positions()
            .insert(position_id, debt_position.clone());

        let liquidate_data = LiquidateData {
            collateral_token: debt_position.collateral_identifier,
            amount,
        };

        Ok(liquidate_data)
    }

    #[payable("EGLD")]
    #[endpoint(issueStablecoinToken)]
    fn issue_stablecoin_token(
        &self,
        #[payment] issue_cost: BigUint,
    ) -> SCResult<AsyncCall<BigUint>> {
        require!(
            self.stablecoin_token_id().is_empty(),
            "Stablecoin already issued"
        );

        self.issue(
            BoxedBytes::from(STABLE_COIN_NAME),
            BoxedBytes::from(STABLE_COIN_TICKER),
            issue_cost,
        )
    }

    #[payable("EGLD")]
    #[endpoint(issueDebtToken)]
    fn issue_debt_token(&self, #[payment] issue_cost: BigUint) -> SCResult<AsyncCall<BigUint>> {
        require!(self.debt_token_id().is_empty(), "Debt token already issued");

        self.issue(
            BoxedBytes::from(DEBT_TOKEN_NAME),
            BoxedBytes::from(DEBT_TOKEN_TICKER),
            issue_cost,
        )
    }

    #[endpoint(setStablecoinRoles)]
    fn set_stablecoin_roles(
        &self,
        #[var_args] roles: VarArgs<EsdtLocalRole>,
    ) -> SCResult<AsyncCall<BigUint>> {
        only_owner!(self, "only owner can set roles");
        require!(
            !self.stablecoin_token_id().is_empty(),
            "token not issued yet"
        );
        Ok(self.set_roles(self.stablecoin_token_id().get(), roles))
    }

    #[endpoint(setBorrowTokenRoles)]
    fn set_borrow_token_roles(
        &self,
        #[var_args] roles: VarArgs<EsdtLocalRole>,
    ) -> SCResult<AsyncCall<BigUint>> {
        only_owner!(self, "only owner can set roles");
        require!(!self.debt_token_id().is_empty(), "token not issued yet");
        Ok(self.set_roles(self.debt_token_id().get(), roles))
    }

    fn issue(
        &self,
        token_display_name: BoxedBytes,
        token_ticker: BoxedBytes,
        issue_cost: BigUint,
    ) -> SCResult<AsyncCall<BigUint>> {
        only_owner!(self, "only owner can issue new tokens");

        Ok(ESDTSystemSmartContractProxy::new()
            .issue_semi_fungible(
                issue_cost,
                &token_display_name,
                &token_ticker,
                SemiFungibleTokenProperties {
                    can_freeze: true,
                    can_wipe: true,
                    can_pause: true,
                    can_change_owner: true,
                    can_upgrade: true,
                    can_add_special_roles: true,
                },
            )
            .async_call()
            .with_callback(self.callbacks().issue_callback(token_ticker)))
    }

    fn set_roles(
        &self,
        token: TokenIdentifier,
        #[var_args] roles: VarArgs<EsdtLocalRole>,
    ) -> AsyncCall<BigUint> {
        ESDTSystemSmartContractProxy::new()
            .set_special_roles(
                &self.get_sc_address(),
                token.as_esdt_identifier(),
                roles.as_slice(),
            )
            .async_call()
    }

    #[callback]
    fn issue_callback(
        &self,
        token_ticker: BoxedBytes,
        #[payment_token] token_identifier: TokenIdentifier,
        #[payment] returned_tokens: BigUint,
        #[call_result] result: AsyncCallResult<()>,
    ) {
        match result {
            AsyncCallResult::Ok(()) => {
                if token_ticker == BoxedBytes::from(STABLE_COIN_TICKER) {
                    self.stablecoin_token_id().set(&token_identifier);
                } else if token_ticker == BoxedBytes::from(DEBT_TOKEN_TICKER) {
                    self.debt_token_id().set(&token_identifier);
                }
            }
            AsyncCallResult::Err(_) => {
                let caller = self.get_owner_address();
                let (returned_tokens, token_id) = self.call_value().payment_token_pair();
                if token_id.is_egld() && returned_tokens > 0 {
                    self.send().direct_egld(&caller, &returned_tokens, &[]);
                }
            }
        }
    }

    fn mint_interest(&self, amount: BigUint, metadata: InterestMetadata) {
        self.send().esdt_nft_create::<InterestMetadata>(
            self.get_gas_left(),
            self.lend_token().get().as_esdt_identifier(),
            &amount,
            &BoxedBytes::empty(),
            &BigUint::zero(),
            &H256::zero(),
            &metadata,
            &[BoxedBytes::empty()],
        )
    }

    fn mint_debt(&self, amount: BigUint, metadata: DebtMetadata<BigUint>, position_id: H256) {
        self.send().esdt_nft_create::<DebtMetadata<BigUint>>(
            self.get_gas_left(),
            self.debt_token_id().get().as_esdt_identifier(),
            &amount,
            &BoxedBytes::empty(),
            &BigUint::zero(),
            &position_id,
            &metadata,
            &[BoxedBytes::empty()],
        );
    }

    fn burn(&self, amount: BigUint, nonce: u64, ticker: TokenIdentifier) {
        self.send().esdt_nft_burn(
            self.get_gas_left(),
            ticker.as_esdt_identifier(),
            nonce,
            &amount,
        );
    }

    fn send_callback_result(&self, token: TokenIdentifier, endpoint: &[u8]) {
        let owner = self.get_owner_address();

        let mut args = ArgBuffer::new();
        args.push_argument_bytes(token.as_esdt_identifier());

        self.send().execute_on_dest_context_raw(
            self.get_gas_left(),
            &owner,
            &BigUint::zero(),
            endpoint,
            &args,
        );
    }

    /// VIEWS

    #[view(getBorrowRate)]
    fn get_borrow_rate(&self) -> BigUint {
        let reserve_data = self.reserve_data().get();
        self._get_borrow_rate(reserve_data, OptionalArg::None)
    }

    #[view(getDepositRate)]
    fn get_deposit_rate(&self) -> BigUint {
        let utilisation = self.get_capital_utilisation();
        let reserve_data = self.reserve_data().get();
        let reserve_factor = reserve_data.reserve_factor.clone();
        let borrow_rate =
            self._get_borrow_rate(reserve_data, OptionalArg::Some(utilisation.clone()));

        self.library_module()
            .compute_deposit_rate(utilisation, borrow_rate, reserve_factor)
    }

    #[view(getDebtInterest)]
    fn get_debt_interest(&self, amount: BigUint, timestamp: u64) -> BigUint {
        let now = self.get_block_timestamp();
        let time_diff = BigUint::from(now - timestamp);

        let borrow_rate = self.get_borrow_rate();

        self.library_module()
            .compute_debt(amount, time_diff, borrow_rate)
    }

    #[view(getCapitalUtilisation)]
    fn get_capital_utilisation(&self) -> BigUint {
        let reserve_amount = self.get_reserve();
        let borrowed_amount = self.get_total_borrow();

        self.library_module()
            .compute_capital_utilisation(borrowed_amount, reserve_amount)
    }

    #[view(getReserve)]
    fn get_reserve(&self) -> BigUint {
        self.reserves()
            .get(&self.pool_asset_id().get())
            .unwrap_or_else(BigUint::zero)
    }

    #[view(poolAsset)]
    fn get_pool_asset(&self) -> TokenIdentifier {
        self.pool_asset_id().get()
    }

    #[view(borrowToken)]
    fn get_borrow_token(&self) -> TokenIdentifier {
        self.debt_token_id().get()
    }

    // UTILS

    fn get_nft_hash(&self) -> H256 {
        let debt_nonce = self.debt_nonce().get();
        let hash = self.keccak256(&debt_nonce.to_be_bytes()[..]);
        self.debt_nonce().set(&(debt_nonce + 1));
        hash
    }

    fn compute_health_factor(&self) -> u32 {
        0u32
    }

    fn _get_borrow_rate(
        &self,
        reserve_data: ReserveData<BigUint>,
        #[var_args] utilisation: OptionalArg<BigUint>,
    ) -> BigUint {
        let u_current = utilisation
            .into_option()
            .unwrap_or_else(|| self.get_capital_utilisation());

        self.library_module().compute_borrow_rate(
            reserve_data.r_base,
            reserve_data.r_slope1,
            reserve_data.r_slope2,
            reserve_data.u_optimal,
            u_current,
        )
    }

    //
    /// stablecoin token id
    #[view(getStablecoinTokenId)]
    #[storage_mapper("stablecoinTokenId")]
    fn stablecoin_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;

    //
    /// pool asset
    #[view(getPoolAssetId)]
    #[storage_mapper("poolAssetId")]
    fn pool_asset_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;

    //
    /// borrow token supported for collateral
    #[view(getDebtTokenId)]
    #[storage_mapper("debtTokenId")]
    fn debt_token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;

    //
    /// pool reserves
    #[storage_mapper("reserves")]
    fn reserves(&self) -> MapMapper<Self::Storage, TokenIdentifier, BigUint>;

    //
    /// debt positions
    #[storage_mapper("debtPositions")]
    fn debt_positions(&self) -> MapMapper<Self::Storage, H256, DebtPosition<BigUint>>;

    //
    /// debt nonce
    #[storage_mapper("debtNonce")]
    fn debt_nonce(&self) -> SingleValueMapper<Self::Storage, u64>;

    //
    /// repay position
    #[storage_mapper("repayPosition")]
    fn repay_position(&self) -> MapMapper<Self::Storage, H256, RepayPostion<BigUint>>;

    //
    /// reserve data
    #[storage_mapper("reserveData")]
    fn reserve_data(&self) -> SingleValueMapper<Self::Storage, ReserveData<BigUint>>;

    //
    /// health factor threshold
    #[view(healthFactorThreshold)]
    #[storage_mapper("healthFactorThreshold")]
    fn health_factor_threshold(&self) -> SingleValueMapper<Self::Storage, u32>;

    //
    // total borrowing from pool
    #[view(getTotalBorrow)]
    #[storage_mapper("totalBorrow")]
    fn total_borrow(&self) -> SingleValueMapper<Self::Storage, BigUint>;
}
