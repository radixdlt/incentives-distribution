use scrypto::prelude::*;

/// An enum storing the vesting configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, ScryptoSbor, ManifestSbor)]
pub enum VestingConfiguration {
    /// Vesting has not yet been initialized and there isn't yet a known vesting
    /// start or end [`Instant`] known. Vesting status remains [`Uninitialized`]
    /// until the `finish_setup` method is called on the vesting component.
    Uninitialized {
        /// The duration of the vesting period in seconds. After this period from
        /// `vest_start`, all tokens will be fully vested (100% available).
        vest_duration_seconds: i64,

        /// The duration of the pre-claim period in seconds. This is the time
        /// between when `finish_setup` is called and when vesting actually begins.
        pre_claim_duration_seconds: i64,

        /// The fraction of tokens that are immediately vested when the vesting
        /// period begins (at `vest_start`). Must be between 0 and 1.
        initial_vested_fraction: Decimal,
    },

    /// An initialized vesting configuration containing the time at which
    /// vesting begins, the time at which vesting ends, and other information
    /// useful for vesting.
    Initialized {
        /// The instant when vesting begins. This is set when `finish_setup` is
        /// called and equals the current time plus the pre-claim duration.
        vest_start: Instant,

        /// The instant when vesting ends and all tokens are fully vested. This
        /// is calculated as `vest_start` plus `vest_duration_seconds` and is set
        /// when `finish_setup` is called.
        vest_end: Instant,

        /// The fraction of tokens that are immediately vested when the vesting
        /// period begins (at `vest_start`). Must be between 0 and 1.
        initial_vested_fraction: Decimal,
    },
}

impl VestingConfiguration {
    /// Creates a new uninitialized vesting configuration with the given parameters.
    pub fn new_uninitialized(
        vest_duration_seconds: i64,
        pre_claim_duration_seconds: i64,
        initial_vested_fraction: Decimal,
    ) -> Self {
        assert!(vest_duration_seconds > 0, "Vest duration must be positive");
        assert!(
            initial_vested_fraction >= Decimal::ZERO && initial_vested_fraction <= Decimal::ONE,
            "initial_vested_fraction must be between 0 and 1"
        );
        assert!(
            pre_claim_duration_seconds >= 0,
            "Pre-claim period must not have negative duration"
        );
        Self::Uninitialized {
            vest_duration_seconds,
            pre_claim_duration_seconds,
            initial_vested_fraction,
        }
    }

    /// Creates a new initialized vesting configuration with the given start
    /// and end times.
    pub fn new_initialized(
        vest_start: Instant,
        vest_end: Instant,
        initial_vested_fraction: Decimal,
    ) -> Self {
        assert!(
            vest_end.seconds_since_unix_epoch >= vest_start.seconds_since_unix_epoch,
            "vest_end must be >= vest_start"
        );
        Self::Initialized {
            vest_start,
            vest_end,
            initial_vested_fraction,
        }
    }

    /// Asserts that vesting is uninitialized.
    pub fn assert_vesting_is_uninitialized(&self) {
        assert!(
            matches!(self, Self::Uninitialized { .. }),
            "Vesting has already been initialized"
        );
    }

    /// Asserts that vesting is initialized.
    pub fn assert_vesting_is_initialized(&self) {
        assert!(
            matches!(self, Self::Initialized { .. }),
            "Vesting has not been initialized yet"
        );
    }
}

/// A wrapper around [`ResolvedVestingState`] that ensures vesting state is
/// always up-to-date before being accessed.
///
/// # Why This Pattern Exists
///
/// The vesting system needs to calculate how many tokens have vested based on
/// elapsed time. Without this wrapper, callers could accidentally read stale
/// values from fields like `vested_tokens` or `pool` if they forgot to call
/// `refill()` first.
///
/// By making the inner state private and only accessible through `get_state()`
/// or `get_state_mut()`, we guarantee at the type system level that `refill()`
/// is always called before any state access. This prevents an entire class of
/// bugs where views return outdated values.
///
/// # Fields Protected by This Wrapper
///
/// The `refill()` method updates:
/// - `vested_tokens` - the cumulative amount of tokens that have vested
/// - `locked_tokens_vault` - tokens are taken from here during vesting
/// - `pool` - vested tokens are deposited here
///
/// Other fields (`vesting_configuration`, `total_tokens_to_vest`) don't change
/// during refill but are included in the wrapper for convenience to avoid
/// passing them as separate parameters.
///
/// Fields that are NOT affected by refill (`locker`, `lp_tokens_vault`) are
/// stored directly on `IncentivesVester` to avoid the refill overhead.
#[derive(ScryptoSbor)]
pub struct VestingState(ResolvedVestingState);

impl VestingState {
    pub fn new(inner: ResolvedVestingState) -> Self {
        Self(inner)
    }

    pub fn get_state(&mut self) -> &ResolvedVestingState {
        self.0.refill();
        &self.0
    }

    pub fn get_state_mut(&mut self) -> &mut ResolvedVestingState {
        self.0.refill();
        &mut self.0
    }
}

#[derive(ScryptoSbor)]
pub struct ResolvedVestingState {
    pub vesting_configuration: VestingConfiguration,
    pub total_tokens_to_vest: Decimal,
    pub vested_tokens: Decimal,
    pub locked_tokens_vault: FungibleVault,
    pub pool: Global<OneResourcePool>,
    pub kill_switch_enabled: bool,
}

impl ResolvedVestingState {
    fn refill(&mut self) {
        // Kill switch is active - don't refill
        if self.kill_switch_enabled {
            return;
        }

        let VestingConfiguration::Initialized {
            vest_start,
            vest_end,
            initial_vested_fraction,
        } = self.vesting_configuration
        else {
            // Vesting not initialized yet - nothing to refill
            return;
        };

        if !Clock::current_time_is_at_or_after(vest_start, TimePrecision::Second) {
            // Still in pre-claim period - nothing to refill yet
            return;
        }

        let current_time = Clock::current_time_rounded_to_seconds();

        let vest_duration = vest_end.seconds_since_unix_epoch
            - vest_start.seconds_since_unix_epoch;

        let elapsed = current_time.seconds_since_unix_epoch
            - vest_start.seconds_since_unix_epoch;

        let raw_progress =
            Decimal::from(elapsed) / Decimal::from(vest_duration);

        let vest_progress = if raw_progress <= Decimal::ZERO {
            Decimal::ZERO
        } else if raw_progress >= Decimal::ONE {
            Decimal::ONE
        } else {
            raw_progress
        };

        let vested_fraction = initial_vested_fraction
            + (Decimal::ONE - initial_vested_fraction) * vest_progress;

        let vested_tokens_target =
            self.total_tokens_to_vest * vested_fraction;

        let tokens_to_vest_now = vested_tokens_target - self.vested_tokens;

        if tokens_to_vest_now <= Decimal::ZERO {
            return;
        }

        let tokens = self.locked_tokens_vault.take(tokens_to_vest_now);
        self.pool.protected_deposit(tokens);

        self.vested_tokens = vested_tokens_target;
    }
}
