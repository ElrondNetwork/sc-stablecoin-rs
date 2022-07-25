elrond_wasm::imports!();
elrond_wasm::derive_imports!();

#[derive(TopEncode, TypeAbi)]
pub struct SwapEvent<M: ManagedTypeApi> {
    pub caller: ManagedAddress<M>,
    pub token_id_in: TokenIdentifier<M>,
    pub token_amount_in: BigUint<M>,
    pub token_id_out: TokenIdentifier<M>,
    pub token_amount_out: BigUint<M>,
    pub fee_amount: BigUint<M>,
    pub block: u64,
    pub epoch: u64,
    pub timestamp: u64,
}

#[derive(TopEncode, TypeAbi)]
pub struct ProvisionEvent<M: ManagedTypeApi> {
    pub caller: ManagedAddress<M>,
    pub token_id_in: TokenIdentifier<M>,
    pub token_amount_in: BigUint<M>,
    pub block: u64,
    pub epoch: u64,
    pub timestamp: u64,
}

#[elrond_wasm::module]
pub trait EventsModule {
    fn emit_swap_event(&self, swap_event: &SwapEvent<Self::Api>) {
        self.swap_event(
            &swap_event.token_id_in,
            &swap_event.token_id_out,
            &swap_event.caller,
            swap_event.epoch,
            swap_event,
        )
    }

    fn emit_provide_collateral_event(&self, provide_collateral_event: &ProvisionEvent<Self::Api>) {
        self.provide_collateral_event(
            &provide_collateral_event.token_id_in,
            &provide_collateral_event.caller,
            provide_collateral_event.epoch,
            provide_collateral_event,
        )
    }

    #[event("swap")]
    fn swap_event(
        &self,
        #[indexed] token_in: &TokenIdentifier,
        #[indexed] token_out: &TokenIdentifier,
        #[indexed] caller: &ManagedAddress,
        #[indexed] epoch: u64,
        swap_event: &SwapEvent<Self::Api>,
    );

    #[event("provide_collateral")]
    fn provide_collateral_event(
        &self,
        #[indexed] token_in: &TokenIdentifier,
        #[indexed] caller: &ManagedAddress,
        #[indexed] epoch: u64,
        provide_collateral_event: &ProvisionEvent<Self::Api>,
    );
}
