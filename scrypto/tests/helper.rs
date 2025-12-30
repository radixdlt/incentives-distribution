#![allow(dead_code)]

use incentives_vester::incentives_vester::incentives_vester_test::*;
use dummy_account::incentives_vester_test::*;
use scrypto_test::prelude::*;

/// Standard tolerance for approximate decimal comparisons in tests
pub const TOLERANCE: Decimal = dec!("0.000000000000001");

pub struct Helper {
    pub env: TestEnvironment<InMemorySubstateDatabase>,
    pub package_address: PackageAddress,
    pub dummy_account_package: PackageAddress,
    pub vester: IncentivesVester,
    pub token_to_vest: Bucket,
    pub admin_badge: Bucket,
    pub super_admin_badge: Bucket,
    pub token_address: ResourceAddress,
    pub admin_badge_address: ResourceAddress,
    pub super_admin_badge_address: ResourceAddress,
    pub lp_resource_address: ResourceAddress,
}

/// Number of seconds in a day (24 * 60 * 60)
pub const SECONDS_PER_DAY: i64 = 86400;

impl Helper {
    pub fn new() -> Result<Self, RuntimeError> {
        // 365 days in seconds, 10% initial vest, 7 days pre-claim in seconds
        Self::new_with_config(365 * SECONDS_PER_DAY, dec!("0.1"), 7 * SECONDS_PER_DAY)
    }

    pub fn new_with_config(
        vest_duration: i64,
        initial_vested_fraction: Decimal,
        pre_claim_duration: i64,
    ) -> Result<Self, RuntimeError> {
        let mut env = TestEnvironmentBuilder::new().build();

        // Create test tokens
        let token_to_vest = ResourceBuilder::new_fungible(OwnerRole::None)
            .divisibility(18)
            .mint_initial_supply(1_000_000, &mut env)?;

        let admin_badge = ResourceBuilder::new_fungible(OwnerRole::None)
            .divisibility(0)
            .mint_initial_supply(1, &mut env)?;

        let super_admin_badge = ResourceBuilder::new_fungible(OwnerRole::None)
            .divisibility(0)
            .mint_initial_supply(1, &mut env)?;

        // Get resource addresses
        let token_address = token_to_vest.resource_address(&mut env)?;
        let admin_badge_address = admin_badge.resource_address(&mut env)?;
        let super_admin_badge_address = super_admin_badge.resource_address(&mut env)?;

        // Compile and publish packages
        let package_address = PackageFactory::compile_and_publish(
            this_package!(),
            &mut env,
            CompileProfile::Fast,
        )?;

        let dummy_account_package = PackageFactory::compile_and_publish(
            "./dummy_account",
            &mut env,
            CompileProfile::Fast,
        )?;

        // Instantiate the IncentivesVester component using the test stub
        // Pass None for locker to create a new one via cross-blueprint call
        let mut vester = IncentivesVester::instantiate(
            admin_badge_address,
            super_admin_badge_address,
            vest_duration,
            initial_vested_fraction,
            pre_claim_duration,
            token_address,
            None,
            package_address,
            &mut env,
        )?;

        // Get the LP resource address (after instantiation, vault exists but is empty)
        let lp_resource_address = vester.get_pool_unit_resource_address(&mut env)?;

        Ok(Self {
            env,
            package_address,
            dummy_account_package,
            vester,
            token_to_vest: token_to_vest.into(),
            admin_badge: admin_badge.into(),
            super_admin_badge: super_admin_badge.into(),
            token_address,
            admin_badge_address,
            super_admin_badge_address,
            lp_resource_address,
        })
    }

    pub fn create_pool_units(&mut self, amount: Decimal) -> Result<(), RuntimeError> {
        let tokens = self.token_to_vest.take(amount, &mut self.env)?;
        let fungible_tokens = FungibleBucket(tokens);

        self.env.disable_auth_module();
        self.vester.create_pool_units(fungible_tokens, &mut self.env)?;
        self.env.enable_auth_module();

        Ok(())
    }

    pub fn finish_setup(&mut self) -> Result<(), RuntimeError> {
        self.env.disable_auth_module();
        self.vester.finish_setup(&mut self.env)?;
        self.env.enable_auth_module();

        Ok(())
    }

    pub fn refill(&mut self) -> Result<(), RuntimeError> {
        self.vester.refill(&mut self.env)?;
        Ok(())
    }

    pub fn get_vested_tokens(&mut self) -> Result<Decimal, RuntimeError> {
        let value = self.vester.get_vested_tokens(&mut self.env)?;
        Ok(value)
    }

    pub fn get_total_tokens_to_vest(&mut self) -> Result<Decimal, RuntimeError> {
        let value = self.vester.get_total_tokens_to_vest(&mut self.env)?;
        Ok(value)
    }

    pub fn get_lp_token_amount(&mut self) -> Result<Decimal, RuntimeError> {
        let amount = self.vester.get_lp_token_amount(&mut self.env)?;
        Ok(amount)
    }

    pub fn get_maturity_value(&mut self) -> Result<Decimal, RuntimeError> {
        let value = self.vester.get_maturity_value(&mut self.env)?;
        Ok(value)
    }

    pub fn claim(&mut self, lp_token_amount: Decimal, account: Reference) -> Result<(), RuntimeError> {
        self.env.disable_auth_module();
        self.vester.claim(lp_token_amount, account, &mut self.env)?;
        self.env.enable_auth_module();
        Ok(())
    }

    pub fn redeem(&mut self, lp_tokens: Bucket) -> Result<Bucket, RuntimeError> {
        let fungible_lp_tokens = FungibleBucket(lp_tokens);
        let redeemed_tokens = self.vester.redeem(fungible_lp_tokens, &mut self.env)?;
        Ok(redeemed_tokens.into())
    }

    pub fn get_pool_vault_amount(&mut self) -> Result<Decimal, RuntimeError> {
        let amount = self.vester.get_pool_vault_amount(&mut self.env)?;
        Ok(amount)
    }

    pub fn get_locked_vault_amount(&mut self) -> Result<Decimal, RuntimeError> {
        let amount = self.vester.get_locked_vault_amount(&mut self.env)?;
        Ok(amount)
    }

    pub fn get_lp_resource_address(&self) -> ResourceAddress {
        self.lp_resource_address
    }

    pub fn create_dummy_account(&mut self) -> Result<(DummyAccount, Reference), RuntimeError> {
        let (dummy_account, account) = DummyAccount::instantiate_account(
            self.dummy_account_package,
            &mut self.env,
        )?;

        Ok((dummy_account, account.into()))
    }

    pub fn get_account_balance(&mut self, dummy_account: &DummyAccount, resource_address: ResourceAddress) -> Result<Decimal, RuntimeError> {
        let balance = dummy_account.balance(resource_address, &mut self.env)?;
        Ok(balance)
    }

    pub fn withdraw_from_account(&mut self, dummy_account: &mut DummyAccount, resource_address: ResourceAddress, amount: Decimal) -> Result<Bucket, RuntimeError> {
        let bucket = dummy_account.withdraw(resource_address, amount, &mut self.env)?;
        Ok(bucket)
    }

    pub fn redeem_lp_from_account(&mut self, dummy_account: &mut DummyAccount, lp_resource_address: ResourceAddress, amount: Decimal) -> Result<Bucket, RuntimeError> {
        // Withdraw LP tokens from the dummy account
        let lp_tokens = dummy_account.withdraw(lp_resource_address, amount, &mut self.env)?;

        // Redeem them through the vester
        let redeemed_tokens = self.redeem(lp_tokens)?;

        Ok(redeemed_tokens)
    }

    pub fn advance_time_days(&mut self, days: i64) {
        let current_time = self.env.get_current_time();
        let new_time = current_time.add_days(days).unwrap();
        self.env.set_current_time(new_time);
    }

    pub fn advance_time_seconds(&mut self, seconds: i64) {
        let current_time = self.env.get_current_time();
        let new_time = current_time.add_seconds(seconds).unwrap();
        self.env.set_current_time(new_time);
    }

    pub fn get_pool_redemption_value(&mut self, lp_amount: Decimal) -> Result<Decimal, RuntimeError> {
        let value = self.vester.get_pool_redemption_value(lp_amount, &mut self.env)?;
        Ok(value)
    }
}

/// Assert that a value is within a tolerance of an expected value
pub fn assert_approx_eq(actual: Decimal, expected: Decimal, tolerance: Decimal, message: &str) {
    let diff = if actual > expected {
        actual - expected
    } else {
        expected - actual
    };

    assert!(
        diff <= tolerance,
        "{}: expected {}, got {}, diff {} (tolerance: {})",
        message,
        expected,
        actual,
        actual - expected,
        tolerance
    );
}
