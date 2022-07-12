use crate::{errors::ERROR_STABLECOIN_TOKEN_NOT_ISSUED, virtual_liquidity_pools::Block};

elrond_wasm::imports!();
elrond_wasm::derive_imports!();

#[derive(TopEncode, TopDecode, PartialEq, TypeAbi)]
pub enum State {
    Inactive,
    Active,
    ActiveNoSwaps,
}

#[elrond_wasm::module]
pub trait ConfigModule {

    #[only_owner]
    #[endpoint(setTokenTicker)]
    fn set_token_ticker(&self, token_id: TokenIdentifier, ticker: ManagedBuffer) {
        self.token_ticker(&token_id).set(&ticker);
    }

    #[only_owner]
    #[endpoint]
    fn pause(&self) {
        self.state().set(&State::Inactive);
    }

    #[only_owner]
    #[endpoint]
    fn resume(&self) {
        require!(
            !self.stablecoin().is_empty(),
            ERROR_STABLECOIN_TOKEN_NOT_ISSUED
        );
        self.state().set(&State::Active);
    }

    #[only_owner]
    #[endpoint(setStateActiveNoSwaps)]
    fn set_state_active_no_swaps(&self) {
        require!(
            !self.stablecoin().is_empty(),
            ERROR_STABLECOIN_TOKEN_NOT_ISSUED
        );
        self.state().set(&State::ActiveNoSwaps);
    }

    #[only_owner]
    #[endpoint(setSpreadFeeMinPercent)]
    fn set_spread_fee_min_percent(&self, spread_fee_percent: BigUint) {
        self.spread_fee_min_percent().set(spread_fee_percent);
    }

    #[only_owner]
    #[endpoint(setPoolRecoveryPeriod)]
    fn set_pool_recovery_period(&self, pool_recovery_period: u64) {
        self.pool_recovery_period().set(pool_recovery_period);
    }
    
    #[inline]
    fn is_state_active(&self) -> bool {
        let state = &self.state().get();
        state == &State::Active || state == &State::ActiveNoSwaps
    }

    #[inline]
    fn can_swap(&self) -> bool {
        let state = &self.state().get();
        state == &State::Active
    }

    #[view(getState)]
    #[storage_mapper("state")]
    fn state(&self) -> SingleValueMapper<State>;

    #[storage_mapper("tokenTicker")]
    fn token_ticker(&self, token_id: &TokenIdentifier) -> SingleValueMapper<ManagedBuffer>;

    #[view(getPriceAggregatorAddress)]
    #[storage_mapper("price_aggregator_address")]
    fn price_aggregator_address(&self) -> SingleValueMapper<ManagedAddress>;

    #[view(getCollateralTokenId)]
    #[storage_mapper("collateral_token_id")]
    fn collateral_token_id(&self) -> SingleValueMapper<TokenIdentifier>;

    #[view(getCollateralSupply)]
    #[storage_mapper("collateral_supply")]
    fn collateral_supply(&self) -> SingleValueMapper<BigUint>;

    #[view(getStablecoinId)]
    #[storage_mapper("stablecoin_id")]
    fn stablecoin(&self) -> FungibleTokenMapper<Self::Api>;

    #[view(getStablecoinSupply)]
    #[storage_mapper("stablecoin_supply")]
    fn stablecoin_supply(&self) -> SingleValueMapper<BigUint>;

    #[view(getBasePool)]
    #[storage_mapper("base_pool")]
    fn base_pool(&self) -> SingleValueMapper<BigUint>;

    #[view(getPoolDelta)]
    #[storage_mapper("pool_delta")]
    fn pool_delta(&self) -> SingleValueMapper<BigUint>;

    #[view(getMedianPoolDelta)]
    #[storage_mapper("median_pool_delta")]
    fn median_pool_delta(&self) -> SingleValueMapper<BigUint>;

    #[view(getPoolRecoveryPeriod)]
    #[storage_mapper("pool_recovery_period")]
    fn pool_recovery_period(&self) -> SingleValueMapper<Block>;

    #[view(getLastReplenishBlock)]
    #[storage_mapper("last_replenish_block")]
    fn last_replenish_block(&self) -> SingleValueMapper<Block>;

    #[view(getSpreadFeeMinPercent)]
    #[storage_mapper("spread_fee_min_percent")]
    fn spread_fee_min_percent(&self) -> SingleValueMapper<BigUint>;

    #[storage_mapper("collateral_tokens")]
    fn collateral_tokens(&self) -> WhitelistMapper<Self::Api, TokenIdentifier>;

    #[view(getCpTokenId)]
    #[storage_mapper("cp_token_id")]
    fn cp_token(&self) -> NonFungibleTokenMapper<Self::Api>;

    #[view(getCpTokenSupply)]
    #[storage_mapper("cp_token_supply")]
    fn cp_token_supply(&self) -> SingleValueMapper<BigUint>;

    #[view(getRewardReserve)]
    #[storage_mapper("reward_reserve")]
    fn reward_reserve(&self) -> SingleValueMapper<BigUint>;

    #[view(getRewardPerShare)]
    #[storage_mapper("reward_per_share")]
    fn reward_per_share(&self) -> SingleValueMapper<BigUint>;

    #[view(getDivisionSafetyConstant)]
    #[storage_mapper("division_safety_constant")]
    fn division_safety_constant(&self) -> SingleValueMapper<BigUint>;
    
}
