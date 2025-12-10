use scrypto::prelude::*;

#[blueprint]
mod incentives_vester {

    enable_method_auth! {
        roles {
            super_admin => updatable_by: [];
            admin => updatable_by: [super_admin];
        },
        methods {
            // Public methods
            refill => PUBLIC;
            redeem => PUBLIC;
            get_maturity_value => PUBLIC;
            get_lp_token_amount => PUBLIC;
            get_pool_vault_amount => PUBLIC;
            get_locked_vault_amount => PUBLIC;
            get_pool_unit_resource_address => PUBLIC;
            get_pool_redemption_value => PUBLIC;
            get_vested_tokens => PUBLIC;
            get_total_tokens_to_vest => PUBLIC;
            // Admin methods
            claim => restrict_to: [super_admin, admin];
            // Super admin methods
            finish_setup => restrict_to: [super_admin];
            create_pool_units => restrict_to: [super_admin];
            remove_lp => restrict_to: [super_admin];
            put_lp => restrict_to: [super_admin];
            put_locked_tokens => restrict_to: [super_admin];
            remove_locked_tokens => restrict_to: [super_admin];
        }
    }

    /// The state and implementation of an incentives vester blueprint.
    ///
    /// The incentives vester blueprint implements a token vesting system that
    /// distributes rewards to users over time. It uses a OneResourcePool to manage
    /// liquidity provider (LP) tokens that represent user claims to vesting rewards.
    ///
    /// The vesting system operates in three distinct phases:
    ///
    /// 1. **Setup Phase**: The super admin deposits tokens into the component and
    ///    creates LP tokens representing future vested rewards. During this phase,
    ///    tokens are freely depositable via `create_pool_units`.
    ///
    /// 2. **Pre-claim Period**: After `finish_setup` is called, a countdown begins
    ///    (e.g., 7 days) during which LP tokens can be distributed to user accounts
    ///    via the `claim` method. Tokens are moved from the pool to the locked vault,
    ///    and vesting has not yet started. Users cannot redeem their LP tokens yet.
    ///    This period protects against potential attacks by ensuring users receive
    ///    their LP tokens before vesting begins.
    ///
    /// 3. **Vesting Period**: After the pre-claim period ends, tokens gradually
    ///    unlock over the configured duration (e.g., 1 year). An initial fraction
    ///    (e.g., 20%) is immediately available. The remaining tokens unlock linearly
    ///    based on elapsed time. Users can redeem their LP tokens at any time,
    ///    receiving the vested portion and forfeiting the unvested portion.
    ///
    /// The component uses an AccountLocker to deliver LP tokens to user accounts
    /// that may have deposit restrictions. The AccountLocker acts as a mailbox
    /// where tokens are stored if an account doesn't allow direct deposits, allowing
    /// users to claim them when ready.
    ///
    /// When users redeem early (before full vesting), they forfeit their unvested
    /// portion. This forfeited amount remains in the pool and increases the maturity
    /// value for remaining LP token holders, creating an incentive to hold until
    /// full vesting.
    struct IncentivesVester {
        /// The account locker component used to deliver LP tokens to user accounts
        /// during the claim process. This circumvents accounts that have deposit
        /// rules configured - if an account doesn't allow direct deposits, the
        /// locker stores the tokens like a mailbox that users can claim from.
        locker: Global<AccountLocker>,

        /// The one-resource pool that manages the vesting tokens and LP tokens.
        /// This pool allows users to redeem their LP tokens for the underlying
        /// vested tokens based on the current vesting progress.
        pool: Global<OneResourcePool>,

        /// A vault holding LP tokens that have not yet been claimed by users.
        /// These tokens are created during setup and distributed to users via
        /// the `claim` method during the pre-claim period.
        lp_tokens_vault: FungibleVault,

        /// A vault holding tokens that are still locked and have not yet vested
        /// into the pool. During the vesting period, tokens are gradually moved
        /// from this vault into the pool via the `refill` method based on the
        /// vesting schedule.
        locked_tokens_vault: FungibleVault,

        /// The total amount of tokens that will be vested over the entire vesting
        /// period. This is set during the setup phase when tokens are deposited
        /// via `create_pool_units` and remains constant throughout vesting.
        total_tokens_to_vest: Decimal,

        /// The cumulative amount of tokens that have been vested so far, meaning
        /// they have been moved from the locked vault into the pool. This value
        /// increases over time as `refill` is called and approaches
        /// `total_tokens_to_vest` as vesting completes.
        vested_tokens: Decimal,

        /// The instant when vesting begins. This is set when `finish_setup` is
        /// called and equals the current time plus the pre-claim duration. It
        /// remains `None` until setup is complete.
        vest_start: Option<Instant>,

        /// The instant when vesting ends and all tokens are fully vested. This
        /// is calculated as `vest_start` plus `vest_duration_days` and is set
        /// when `finish_setup` is called. It remains `None` until setup is complete.
        vest_end: Option<Instant>,

        /// The duration of the vesting period in days. After this period from
        /// `vest_start`, all tokens will be fully vested (100% available). This
        /// is set during instantiation and cannot be changed.
        vest_duration_days: i64,

        /// The duration of the pre-claim period in seconds. This is the time
        /// between when `finish_setup` is called and when vesting actually begins.
        /// During this period, LP tokens can be distributed to users but cannot
        /// be redeemed yet. This is set during instantiation and cannot be changed.
        pre_claim_duration_seconds: i64,

        /// The fraction of tokens that are immediately vested when the vesting
        /// period begins (at `vest_start`). This must be between 0 and 1. For
        /// example, 0.1 means 10% of tokens are immediately accessible when
        /// vesting starts. The remaining tokens vest linearly over the vesting
        /// duration. This is set during instantiation and cannot be changed.
        initial_vested_fraction: Decimal,
    }

    impl IncentivesVester {
        /// Instantiates a new incentives vester component for the given token
        /// and vesting parameters.
        ///
        /// This function creates a new incentives vester component that will
        /// distribute the specified token to users over time according to a
        /// vesting schedule. The component uses a OneResourcePool to manage
        /// LP tokens and an AccountLocker to securely distribute them to users.
        ///
        /// The vesting schedule consists of an initial immediately vested
        /// fraction plus linear vesting of the remainder over the specified
        /// duration. For example, with `initial_vested_fraction = 0.1` and
        /// `vest_duration_days = 365`, users will have access to 10% of their
        /// tokens immediately when vesting starts, and the remaining 90% will
        /// unlock linearly over 365 days.
        ///
        /// # Arguments
        ///
        /// - `admin_badge_address`: [`ResourceAddress`] - The address of the
        ///   admin badge resource. Holders of this badge can claim LP tokens
        ///   for users via the `claim` method. This is typically held by a
        ///   backend service that distributes rewards.
        /// - `super_admin_badge_address`: [`ResourceAddress`] - The address of
        ///   the super admin badge resource. Holders of this badge have full
        ///   control over the component, including depositing tokens, finishing
        ///   setup, and withdrawing tokens if needed.
        /// - `vest_duration_days`: [`i64`] - The duration of the vesting period
        ///   in days. After this period from `vest_start`, all tokens will be
        ///   fully vested. Must be positive.
        /// - `initial_vested_fraction`: [`Decimal`] - The fraction of tokens
        ///   that are immediately vested when the vesting period begins. Must
        ///   be between 0 and 1. For example, 0.2 means 20% of tokens are
        ///   immediately accessible.
        /// - `pre_claim_duration_seconds`: [`i64`] - The duration of the
        ///   pre-claim period in seconds. This is the time between when
        ///   `finish_setup` is called and when vesting actually begins. During
        ///   this period, LP tokens can be distributed but not redeemed. Must
        ///   be non-negative.
        /// - `token_to_vest`: [`ResourceAddress`] - The address of the fungible
        ///   token resource that will be vested to users.
        /// - `dapp_def_address`: [`ComponentAddress`] - The dapp definition
        ///   address for metadata purposes.
        ///
        /// # Returns
        ///
        /// - [`Global<IncentivesVester>`] - The global address of the component
        ///   that was instantiated through this function.
        ///
        /// # Panics
        ///
        /// This function will panic if:
        /// - `vest_duration_days` is not positive
        /// - `initial_vested_fraction` is not between 0 and 1
        /// - `pre_claim_duration_seconds` is negative
        pub fn instantiate(
            admin_badge_address: ResourceAddress,
            super_admin_badge_address: ResourceAddress,
            vest_duration_days: i64,
            initial_vested_fraction: Decimal,
            pre_claim_duration_seconds: i64,
            token_to_vest: ResourceAddress,
            dapp_def_address: ComponentAddress,
        ) -> Global<IncentivesVester> {
            let (address_reservation, component_address) =
                Runtime::allocate_component_address(IncentivesVester::blueprint_id());

            assert!(vest_duration_days > 0, "Vest duration must be positive");
            assert!(
                initial_vested_fraction >= Decimal::ZERO && initial_vested_fraction <= Decimal::ONE,
                "initial_vested_fraction must be between 0 and 1"
            );
            assert!(
                pre_claim_duration_seconds >= 0,
                "Pre-claim period must not have negative duration."
            );

            let admin_access_rule = rule!(require(admin_badge_address));

            let super_admin_access_rule = rule!(
                require(super_admin_badge_address) || require(global_caller(component_address))
            );
            let super_admin_owner_role = OwnerRole::Fixed(super_admin_access_rule.clone());

            let locker = Blueprint::<AccountLocker>::instantiate(
                super_admin_owner_role.clone(),
                super_admin_access_rule.clone(),
                super_admin_access_rule.clone(),
                super_admin_access_rule.clone(),
                super_admin_access_rule.clone(),
                None,
            );

            let pool = Blueprint::<OneResourcePool>::instantiate(
                super_admin_owner_role.clone(),
                super_admin_access_rule,
                token_to_vest,
                None,
            );

            let pool_unit_global_address: GlobalAddress =
                pool.get_metadata("pool_unit").unwrap().unwrap();
            let pool_unit_resource_address =
                ResourceAddress::try_from(pool_unit_global_address).unwrap();

            // We can set the metadata of the pool unit here immediately.
            // But we would need to pass the super_admin_badge at instantiation to allow that.
            // Let's not for now.

            Self {
                locker,
                pool,

                // Vault that will hold the pool units the users can claim
                lp_tokens_vault: FungibleVault::new(pool_unit_resource_address),

                // Vault that will be filled with tokens to vest (that are still unvested)
                locked_tokens_vault: FungibleVault::new(token_to_vest),

                // Already vested amount = initial immediate vest
                vested_tokens: Decimal::ZERO,
                total_tokens_to_vest: Decimal::ZERO,

                // Vest will only start once all lp tokens have been created. This will them turn into a Some.
                vest_start: None,
                vest_end: None,

                // Vesting parameters

                // Duration of vest in days
                vest_duration_days,
                // Pre-claim duration in seconds
                pre_claim_duration_seconds,
                // Amount of tokens users can immediately access from the start of the vest.
                initial_vested_fraction,
            }
            .instantiate()
            .prepare_to_globalize(super_admin_owner_role)
            .roles(roles! {
                super_admin => OWNER;
                admin => admin_access_rule;
            })
            .with_address(address_reservation)
            .metadata(metadata! {
                init {
                    "name" => "Incentives Vester".to_string(), updatable;
                    "dapp_definition" => dapp_def_address, updatable;
                }
            })
            .globalize()
        }

        // region:Super Admin Methods

        /// Deposits tokens into the pool and creates corresponding LP tokens.
        ///
        /// This method is used during the setup phase to fill the component with
        /// tokens that will be vested to users. It can be called multiple times
        /// before `finish_setup` is called to add tokens incrementally.
        ///
        /// The tokens are deposited into the OneResourcePool, which mints LP tokens
        /// in return. These LP tokens represent claims to the vested tokens and will
        /// be distributed to users via the `claim` method during the pre-claim period.
        ///
        /// The amount of tokens deposited is tracked in `total_tokens_to_vest` and
        /// determines the total amount that will be vested over the vesting period.
        ///
        /// # Arguments
        ///
        /// - `tokens_to_vest`: [`FungibleBucket`] - A bucket containing the tokens
        ///   to add to the vesting pool. These will be vested to users over time.
        ///
        /// # Panics
        ///
        /// This method will panic if called after `finish_setup` has been called,
        /// as setup can only occur before the vesting process begins.
        pub fn create_pool_units(&mut self, tokens_to_vest: FungibleBucket) {
            assert!(self.vest_start.is_none(), "Vesting has already started");

            // Track the actual amount of tokens contributed
            let amount = tokens_to_vest.amount();
            self.total_tokens_to_vest += amount;

            let lp_tokens = self.pool.contribute(tokens_to_vest);
            self.lp_tokens_vault.put(lp_tokens);
        }

        /// Finalizes the setup phase and begins the pre-claim period.
        ///
        /// This method transitions the component from the setup phase to the
        /// pre-claim period. It moves all tokens from the pool into the locked
        /// vault and sets the vesting start and end times.
        ///
        /// After this method is called:
        /// - The pre-claim period begins, lasting `pre_claim_duration_seconds`
        /// - During the pre-claim period, LP tokens can be claimed by users via
        ///   the `claim` method, but users cannot redeem them yet
        /// - When the pre-claim period ends, vesting begins and users can start
        ///   redeeming their LP tokens for the vested portion
        /// - No more tokens can be added via `create_pool_units`
        ///
        /// The vesting schedule is configured as follows:
        /// - `vest_start` = current_time + `pre_claim_duration_seconds`
        /// - `vest_end` = `vest_start` + `vest_duration_days`
        ///
        /// # Panics
        ///
        /// This method will panic if called more than once, as setup can only
        /// be finalized once.
        pub fn finish_setup(&mut self) {
            assert!(self.vest_start.is_none(), "Vesting has already started");

            let current_time = Clock::current_time_rounded_to_seconds();
            let pre_claim_end = current_time
                .add_seconds(self.pre_claim_duration_seconds)
                .unwrap();

            self.vest_start = Some(pre_claim_end);
            self.vest_end = Some(pre_claim_end.add_days(self.vest_duration_days).unwrap());

            let tokens_to_unvest = self.pool.get_vault_amount();

            let unvested_tokens = self.pool.protected_withdraw(
                tokens_to_unvest,
                WithdrawStrategy::Rounded(RoundingMode::ToZero),
            );

            self.locked_tokens_vault.put(unvested_tokens);
        }

        /// Removes all LP tokens from the component's internal vault.
        ///
        /// This method withdraws all LP tokens that have not yet been claimed
        /// by users. It does NOT affect LP tokens that have already been
        /// distributed to user accounts via the `claim` method.
        ///
        /// This is an emergency function that allows the super admin to recover
        /// unclaimed LP tokens if needed. Use with caution as it can affect the
        /// ability to distribute rewards to users.
        ///
        /// # Returns
        ///
        /// - [`FungibleBucket`] - A bucket containing all LP tokens from the vault.
        pub fn remove_lp(&mut self) -> FungibleBucket {
            self.lp_tokens_vault.take_all()
        }

        /// Deposits LP tokens back into the component's internal vault.
        ///
        /// This method returns LP tokens to the component's vault, making them
        /// available for distribution to users via the `claim` method.
        ///
        /// This is typically used in conjunction with `remove_lp` to temporarily
        /// withdraw and then return LP tokens.
        ///
        /// # Arguments
        ///
        /// - `tokens`: [`FungibleBucket`] - A bucket containing the LP tokens
        ///   to deposit into the vault.
        pub fn put_lp(&mut self, tokens: FungibleBucket) {
            self.lp_tokens_vault.put(tokens)
        }

        /// Removes all locked (unvested) tokens from the component.
        ///
        /// This method withdraws all tokens that are still in the locked vault
        /// and have not yet been vested into the pool. This will affect future
        /// vesting as these tokens will no longer be available to vest.
        ///
        /// This is an emergency function that allows the super admin to recover
        /// unvested tokens if needed. Use with extreme caution as it will prevent
        /// users from receiving their full vested amount.
        ///
        /// # Returns
        ///
        /// - [`FungibleBucket`] - A bucket containing all locked tokens.
        pub fn remove_locked_tokens(&mut self) -> FungibleBucket {
            self.locked_tokens_vault.take_all()
        }

        /// Deposits locked tokens back into the component's vault.
        ///
        /// This method returns locked tokens to the component's vault, making them
        /// available for vesting according to the vesting schedule.
        ///
        /// This is typically used in conjunction with `remove_locked_tokens` to
        /// temporarily withdraw and then return locked tokens.
        ///
        /// # Arguments
        ///
        /// - `tokens`: [`FungibleBucket`] - A bucket containing the tokens to
        ///   deposit into the locked vault.
        pub fn put_locked_tokens(&mut self, tokens: FungibleBucket) {
            self.locked_tokens_vault.put(tokens)
        }

        // endregion:Super Admin Methods

        // region:Admin Methods

        /// Claims LP tokens for a user and deposits them into their account.
        ///
        /// This method distributes LP tokens to a user's account during the
        /// pre-claim period or after vesting has started. The LP tokens are
        /// deposited using the AccountLocker, which acts as a mailbox for accounts
        /// that have deposit restrictions. If the account doesn't allow direct
        /// deposits, the tokens are stored in the locker where the user can claim
        /// them.
        ///
        /// This method is typically called by a backend service that holds the
        /// admin badge and distributes rewards to users based on their activity
        /// or participation in an incentives program.
        ///
        /// # Arguments
        ///
        /// - `lp_token_amount`: [`Decimal`] - The amount of LP tokens to claim
        ///   for the user. Must be greater than zero.
        /// - `account_address`: [`Global<Account>`] - The account address where
        ///   the LP tokens will be deposited.
        ///
        /// # Panics
        ///
        /// This method will panic if:
        /// - Called before `finish_setup` has been called
        /// - `lp_token_amount` is zero or negative
        pub fn claim(&mut self, lp_token_amount: Decimal, account_address: Global<Account>) {
            assert!(self.vest_start.is_some(), "Vesting not set up yet.");

            assert!(
                lp_token_amount > Decimal::ZERO,
                "LP token amount must be greater than zero"
            );

            let lp_tokens = self.lp_tokens_vault.take(lp_token_amount);
            self.locker.store(account_address, lp_tokens.into(), true);

            // Potentially, we can mint an NFT here to represent the user's performance in Season 1
            // We would also deposit it with the account_locker
        }

        // endregion:Admin Methods

        // region:Public Methods

        /// Moves vested tokens from the locked vault into the pool.
        ///
        /// This method calculates how many tokens should have vested based on
        /// the current time and the vesting schedule, then moves those tokens
        /// from the locked vault into the pool, making them available for
        /// redemption.
        ///
        /// The vesting calculation uses a linear schedule with an initial vested
        /// fraction:
        /// - At `vest_start` (0% progress): `initial_vested_fraction` is available
        /// - During vesting: Linear interpolation between initial and 100%
        /// - At `vest_end` (100% progress): All tokens are available
        ///
        /// Formula: `vested_fraction = initial_vested_fraction + (1 - initial_vested_fraction) * progress`
        ///
        /// This method is idempotent - calling it multiple times at the same
        /// point in time will not move additional tokens. It automatically gets
        /// called during `redeem`, but can also be called manually to update
        /// the pool and show accurate LP token values in wallets.
        ///
        /// # Panics
        ///
        /// This method will panic if:
        /// - Called before `finish_setup` has been called
        /// - Called during the pre-claim period (before `vest_start`)
        pub fn refill(&mut self) {
            if let Some(vest_start) = self.vest_start {
                assert!(
                    Clock::current_time_is_at_or_after(vest_start, TimePrecision::Second),
                    "Still in pre-claim period. Vesting not started yet."
                );
            } else {
                panic!("Vesting setup not complete yet.");
            }

            let current_time = Clock::current_time_rounded_to_seconds();

            let vest_duration = self.vest_end.unwrap().seconds_since_unix_epoch
                - self.vest_start.unwrap().seconds_since_unix_epoch;

            let elapsed = current_time.seconds_since_unix_epoch
                - self.vest_start.unwrap().seconds_since_unix_epoch;

            let raw_progress = Decimal::from(elapsed) / Decimal::from(vest_duration);

            let vest_progress = if raw_progress <= Decimal::ZERO {
                Decimal::ZERO
            } else if raw_progress >= Decimal::ONE {
                Decimal::ONE
            } else {
                raw_progress
            };

            // Apply initial vested fraction + linear vesting of the remainder
            // At vest_start (progress = 0): initial_vested_fraction is available
            // At vest_end (progress = 1): 100% is available
            // Formula: initial + (1 - initial) * progress
            let vested_fraction = self.initial_vested_fraction
                + (Decimal::ONE - self.initial_vested_fraction) * vest_progress;

            // Target total vested amount at this point in time
            let vested_tokens_target = self.total_tokens_to_vest * vested_fraction;

            let tokens_to_vest_now = vested_tokens_target - self.vested_tokens;

            if tokens_to_vest_now <= Decimal::ZERO {
                return;
            }

            let tokens = self.locked_tokens_vault.take(tokens_to_vest_now);
            self.pool.protected_deposit(tokens);

            self.vested_tokens = vested_tokens_target;
        }

        /// Redeems LP tokens for the vested portion of the underlying tokens.
        ///
        /// This method allows users to exchange their LP tokens for the tokens
        /// that have vested so far. Users receive a proportional share of the
        /// currently vested tokens based on their LP token amount, and forfeit
        /// their claim to any unvested tokens.
        ///
        /// The redemption value is calculated by the OneResourcePool based on the
        /// ratio of vested tokens in the pool to the total LP token supply. When
        /// users redeem early (before 100% vesting), they forfeit their unvested
        /// portion, which remains in the pool and increases the maturity value for
        /// remaining LP token holders.
        ///
        /// This method automatically calls `refill` before redemption to ensure
        /// the pool is up-to-date with the current vesting progress.
        ///
        /// # Arguments
        ///
        /// - `lp_token_bucket`: [`FungibleBucket`] - A bucket containing the LP
        ///   tokens to redeem. Must contain at least some amount.
        ///
        /// # Returns
        ///
        /// - [`FungibleBucket`] - A bucket containing the vested tokens received
        ///   in exchange for the LP tokens.
        ///
        /// # Panics
        ///
        /// This method will panic if the LP token bucket is empty (contains zero
        /// tokens).
        pub fn redeem(&mut self, lp_token_bucket: FungibleBucket) -> FungibleBucket {
            assert!(
                lp_token_bucket.amount() > Decimal::ZERO,
                "LP bucket must contain some amount"
            );
            self.refill();
            self.pool.redeem(lp_token_bucket)
        }

        /// Returns the amount of LP tokens in the component's internal vault.
        ///
        /// This method returns the amount of LP tokens that have not yet been
        /// claimed by users. It does not include LP tokens that have already
        /// been distributed to user accounts.
        ///
        /// # Returns
        ///
        /// - [`Decimal`] - The amount of unclaimed LP tokens in the vault.
        pub fn get_lp_token_amount(&mut self) -> Decimal {
            self.lp_tokens_vault.amount()
        }

        /// Returns the projected value of 1 LP token at full maturity.
        ///
        /// This method calculates what 1 LP token will be worth when all tokens
        /// are fully vested (at `vest_end`). This is useful for showing users
        /// the potential future value of their LP tokens.
        ///
        /// The maturity value increases when users redeem early, as they forfeit
        /// their unvested portion which remains in the pool for other LP token
        /// holders. This creates an incentive to hold LP tokens until full vesting.
        ///
        /// The calculation is:
        /// `maturity_value = (pool_tokens + locked_tokens) / pool_tokens * current_redemption_value`
        ///
        /// This method calls `refill` first to ensure the pool is up-to-date.
        ///
        /// # Returns
        ///
        /// - [`Decimal`] - The projected value of 1 LP token at full maturity.
        ///
        /// # Panics
        ///
        /// This method will panic if the current redemption value is 0, which
        /// should only occur if the pool is empty.
        pub fn get_maturity_value(&mut self) -> Decimal {
            self.refill();

            let current_redemption_value = self.pool.get_redemption_value(Decimal::ONE);

            let current_unlocked_amount = self.pool.get_vault_amount();
            let still_locked_amount = self.locked_tokens_vault.amount();

            let final_token_amount = current_unlocked_amount + still_locked_amount;

            let maturity_factor = final_token_amount / current_unlocked_amount;

            maturity_factor * current_redemption_value
        }

        /// Returns the amount of tokens currently in the pool.
        ///
        /// This method returns the amount of vested tokens that are currently
        /// available for redemption in the pool. This amount increases over time
        /// as tokens are vested via the `refill` method.
        ///
        /// # Returns
        ///
        /// - [`Decimal`] - The amount of tokens in the pool vault.
        pub fn get_pool_vault_amount(&mut self) -> Decimal {
            self.pool.get_vault_amount()
        }

        /// Returns the amount of tokens still locked (not yet vested).
        ///
        /// This method returns the amount of tokens in the locked vault that
        /// have not yet been vested into the pool. This amount decreases over
        /// time as tokens are vested via the `refill` method.
        ///
        /// # Returns
        ///
        /// - [`Decimal`] - The amount of locked tokens.
        pub fn get_locked_vault_amount(&mut self) -> Decimal {
            self.locked_tokens_vault.amount()
        }

        /// Returns the resource address of the LP tokens.
        ///
        /// This method returns the resource address of the LP tokens that are
        /// minted by the pool and represent claims to vested tokens. Users need
        /// this address to identify their LP tokens in their wallets.
        ///
        /// # Returns
        ///
        /// - [`ResourceAddress`] - The resource address of the LP tokens.
        pub fn get_pool_unit_resource_address(&self) -> ResourceAddress {
            self.lp_tokens_vault.resource_address()
        }

        /// Returns the current redemption value for a given amount of LP tokens.
        ///
        /// This method calculates how many tokens would be received if the
        /// specified amount of LP tokens were redeemed at the current time.
        /// The value depends on how much has vested so far and how many LP
        /// tokens have already been redeemed.
        ///
        /// Note that this returns the value at the current moment. To get an
        /// up-to-date value that includes the latest vesting progress, call
        /// `refill` first or use this after a `redeem` call (which automatically
        /// calls `refill`).
        ///
        /// # Arguments
        ///
        /// - `lp_amount`: [`Decimal`] - The amount of LP tokens to calculate
        ///   the redemption value for.
        ///
        /// # Returns
        ///
        /// - [`Decimal`] - The amount of tokens that would be received for
        ///   redeeming the specified amount of LP tokens.
        pub fn get_pool_redemption_value(&self, lp_amount: Decimal) -> Decimal {
            self.pool.get_redemption_value(lp_amount)
        }

        /// Returns the total amount of tokens that have been vested so far.
        ///
        /// This method returns the cumulative amount of tokens that have been
        /// moved from the locked vault into the pool through the `refill` method.
        /// This value increases over time and approaches `total_tokens_to_vest`
        /// as vesting progresses.
        ///
        /// # Returns
        ///
        /// - [`Decimal`] - The amount of tokens that have been vested.
        pub fn get_vested_tokens(&self) -> Decimal {
            self.vested_tokens
        }

        /// Returns the total amount of tokens that will be vested over the
        /// entire vesting period.
        ///
        /// This method returns the total amount of tokens that were deposited
        /// via `create_pool_units` during the setup phase. This value is set
        /// during setup and remains constant throughout the vesting period.
        ///
        /// # Returns
        ///
        /// - [`Decimal`] - The total amount of tokens to vest.
        pub fn get_total_tokens_to_vest(&self) -> Decimal {
            self.total_tokens_to_vest
        }

        // endregion:Public Methods
    }
}
