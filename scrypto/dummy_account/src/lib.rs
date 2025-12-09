use scrypto::prelude::*;

#[blueprint]
mod incentives_vester {
    struct DummyAccount {
        account: Global<Account>,
    }

    impl DummyAccount {
        pub fn instantiate_account() -> (Global<DummyAccount>, Global<Account>) {
            let account =
                Blueprint::<Account>::create_advanced(OwnerRole::Fixed(rule!(allow_all)), None);

            let component = Self { account }
                .instantiate()
                .prepare_to_globalize(OwnerRole::Fixed(rule!(allow_all)))
                .globalize();

            (component, account)
        }

        pub fn balance(&self, address: ResourceAddress) -> Decimal {
            self.account.balance(address)
        }

        pub fn withdraw(&mut self, address: ResourceAddress, amount: Decimal) -> Bucket {
            self.account.withdraw(address, amount)
        }
    }
}
