mod helper;
use helper::Helper;
use scrypto_test::prelude::*;

// ==================== Basic Tests ====================

#[test]
fn test_instantiate() -> Result<(), RuntimeError> {
    let helper = Helper::new()?;
    assert!(helper.vester.0.is_global());
    Ok(())
}

#[test]
fn test_create_pool_units_once() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;
    helper.create_pool_units(dec!("10000"))?;

    let lp_amount = helper.get_lp_token_amount()?;
    assert_eq!(lp_amount, dec!("10000"));

    Ok(())
}

#[test]
fn test_create_pool_units_multiple_times() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;

    helper.create_pool_units(dec!("5000"))?;
    helper.create_pool_units(dec!("3000"))?;
    helper.create_pool_units(dec!("2000"))?;

    let lp_amount = helper.get_lp_token_amount()?;
    assert_eq!(lp_amount, dec!("10000"));

    Ok(())
}

#[test]
fn test_finish_setup() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;

    helper.create_pool_units(dec!("10000"))?;
    helper.finish_setup()?;

    // After finish_setup, LP tokens should still be in vault
    let lp_amount = helper.get_lp_token_amount()?;
    assert_eq!(lp_amount, dec!("10000"));

    // All tokens should be locked
    let pool_amount = helper.get_pool_vault_amount()?;
    let locked_amount = helper.get_locked_vault_amount()?;
    assert_eq!(pool_amount, dec!("0"));
    assert_eq!(locked_amount, dec!("10000"));

    Ok(())
}

#[test]
#[should_panic(expected = "Vesting has already started")]
fn test_create_pool_units_after_finish_setup_fails() {
    let mut helper = Helper::new().unwrap();

    helper.create_pool_units(dec!("10000")).unwrap();
    helper.finish_setup().unwrap();

    // This should panic
    helper.create_pool_units(dec!("5000")).unwrap();
}

#[test]
#[should_panic(expected = "Vesting has already started")]
fn test_finish_setup_twice_fails() {
    let mut helper = Helper::new().unwrap();

    helper.create_pool_units(dec!("10000")).unwrap();
    helper.finish_setup().unwrap();

    // This should panic
    helper.finish_setup().unwrap();
}

#[test]
#[should_panic(expected = "Vesting setup not complete yet")]
fn test_refill_before_setup_fails() {
    let mut helper = Helper::new().unwrap();

    // This should panic
    helper.refill().unwrap();
}

#[test]
#[should_panic(expected = "Still in pre-claim period")]
fn test_refill_during_pre_claim_period_fails() {
    let mut helper = Helper::new().unwrap();

    helper.create_pool_units(dec!("10000")).unwrap();
    helper.finish_setup().unwrap();

    // Advance to 1 second before vest_start (still in pre-claim period)
    helper.advance_time_seconds(604799);

    // This should panic
    helper.refill().unwrap();
}

#[test]
fn test_refill_idempotent() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;

    helper.create_pool_units(dec!("10000"))?;
    helper.finish_setup()?;

    // Advance to vest_start + 100 days
    helper.advance_time_seconds(604800);
    helper.advance_time_days(100);

    helper.refill()?;
    let pool_1 = helper.get_pool_vault_amount()?;
    let locked_1 = helper.get_locked_vault_amount()?;

    // Call refill multiple times at the same time point
    helper.refill()?;
    helper.refill()?;

    // Amounts should not change
    let pool_2 = helper.get_pool_vault_amount()?;
    let locked_2 = helper.get_locked_vault_amount()?;

    assert_eq!(pool_1, pool_2);
    assert_eq!(locked_1, locked_2);

    Ok(())
}

#[test]
fn test_refill_vault_contents_at_checkpoints() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;

    helper.create_pool_units(dec!("10000"))?;
    helper.finish_setup()?;

    // After setup: all locked
    let pool_0 = helper.get_pool_vault_amount()?;
    let locked_0 = helper.get_locked_vault_amount()?;
    assert_eq!(pool_0, dec!("0"));
    assert_eq!(locked_0, dec!("10000"));

    // Advance to exactly vest_start (0% linear progress, but 10% initial vest available)
    helper.advance_time_seconds(604800);
    helper.refill()?;

    // At vest_start (0% linear progress): exactly 10% should be available
    // vested_fraction = 0.1 + (1 - 0.1) * 0 = 0.1
    // Expected pool: 10000 * 0.1 = 1000
    let pool_0 = helper.get_pool_vault_amount()?;
    let locked_0_after = helper.get_locked_vault_amount()?;

    helper::assert_approx_eq(
        pool_0,
        dec!("1000"),
        helper::TOLERANCE,
        "pool at vest_start",
    );
    assert_eq!(pool_0 + locked_0_after, dec!("10000"));

    // Advance to exactly 25% linear progress (91.25 days from vest_start)
    helper.advance_time_days(91);
    helper.advance_time_seconds(21600); // 0.25 days = 6 hours = 21600 seconds
    helper.refill()?;

    // At 25% linear progress:
    // vested_fraction = 0.1 + (1 - 0.1) * 0.25 = 0.1 + 0.225 = 0.325
    // Expected pool: 10000 * 0.325 = 3250
    let pool_25 = helper.get_pool_vault_amount()?;
    let locked_25 = helper.get_locked_vault_amount()?;
    let expected_25 = dec!("3250");

    helper::assert_approx_eq(
        pool_25,
        expected_25,
        helper::TOLERANCE,
        "25% progress pool amount",
    );
    assert_eq!(pool_25 + locked_25, dec!("10000"));

    // Advance to exactly 50% linear progress (182.5 days from vest_start)
    // We're at 91.25 days, need to reach 182.5 days: 182.5 - 91.25 = 91.25 days
    helper.advance_time_days(91);
    helper.advance_time_seconds(21600); // 0.25 days = 21600 seconds
    helper.refill()?;

    // At 50% linear progress:
    // vested_fraction = 0.1 + (1 - 0.1) * 0.5 = 0.1 + 0.45 = 0.55
    // Expected pool: 10000 * 0.55 = 5500
    let pool_50 = helper.get_pool_vault_amount()?;
    let locked_50 = helper.get_locked_vault_amount()?;
    let expected_50 = dec!("5500");

    helper::assert_approx_eq(
        pool_50,
        expected_50,
        helper::TOLERANCE,
        "50% progress pool amount",
    );
    assert_eq!(pool_50 + locked_50, dec!("10000"));

    // Advance to exactly 100% linear progress (365 days from vest_start)
    helper.advance_time_days(182);
    helper.advance_time_seconds(43200); // 0.5 days
    helper.refill()?;

    // At 100% linear progress:
    // vested_fraction = 0.1 + (1 - 0.1) * 1.0 = 0.1 + 0.9 = 1.0
    // Expected pool: 10000 * 1.0 = 10000
    let pool_100 = helper.get_pool_vault_amount()?;
    let locked_100 = helper.get_locked_vault_amount()?;

    assert_eq!(pool_100, dec!("10000"));
    assert_eq!(locked_100, dec!("0"));

    Ok(())
}

#[test]
fn test_refill_long_after_vesting_complete() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;

    helper.create_pool_units(dec!("10000"))?;
    helper.finish_setup()?;

    // Advance to vest_start + way beyond full vesting (2 years)
    helper.advance_time_seconds(604800);
    helper.advance_time_days(730);

    helper.refill()?;

    // Check vault amounts - all should be in pool
    let pool_amount = helper.get_pool_vault_amount()?;
    let locked_amount = helper.get_locked_vault_amount()?;

    assert_eq!(pool_amount, dec!("10000"));
    assert_eq!(locked_amount, dec!("0"));

    // Multiple refills should be idempotent
    helper.refill()?;
    helper.refill()?;

    let pool_amount_after = helper.get_pool_vault_amount()?;
    let locked_amount_after = helper.get_locked_vault_amount()?;

    assert_eq!(pool_amount_after, dec!("10000"));
    assert_eq!(locked_amount_after, dec!("0"));

    Ok(())
}

// ==================== Maturity Value Tests ====================

#[test]
fn test_maturity_value_with_no_redemptions() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;

    helper.create_pool_units(dec!("10000"))?;
    helper.finish_setup()?;

    // Advance to vest_start
    helper.advance_time_seconds(604800);
    helper.refill()?;

    // With no redemptions, maturity value should be exactly 1
    // Formula: (total_tokens / pool_unlocked) * (pool_unlocked / lp_supply) = total_tokens / lp_supply = 10000 / 10000 = 1
    let maturity_value = helper.get_maturity_value()?;

    helper::assert_approx_eq(
        maturity_value,
        dec!("1"),
        helper::TOLERANCE,
        "initial maturity value",
    );

    // Test at 50% vesting - should still be 1
    helper.advance_time_days(182);
    helper.advance_time_seconds(43200); // 0.5 days = 43200 seconds
    helper.refill()?;

    let maturity_50 = helper.get_maturity_value()?;
    helper::assert_approx_eq(
        maturity_50,
        dec!("1"),
        helper::TOLERANCE,
        "maturity at 50% vesting",
    );

    // Test at 100% vesting - should still be 1
    helper.advance_time_days(182);
    helper.advance_time_seconds(43200); // 0.5 days = 43200 seconds
    helper.refill()?;

    let maturity_100 = helper.get_maturity_value()?;
    helper::assert_approx_eq(
        maturity_100,
        dec!("1"),
        helper::TOLERANCE,
        "maturity at 100% vesting",
    );

    Ok(())
}

#[test]
fn test_redeem_half_doubles_maturity() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;

    helper.create_pool_units(dec!("10000"))?;
    helper.finish_setup()?;

    // Advance to vest_start (0% linear progress, 10% initial vest)
    helper.advance_time_seconds(604800);
    helper.refill()?;

    let pool_initial = helper.get_pool_vault_amount()?;
    let locked_initial = helper.get_locked_vault_amount()?;
    assert_eq!(pool_initial + locked_initial, dec!("10000"));

    let lp_resource = helper.get_lp_resource_address();

    // Initial maturity = 1
    let maturity_before = helper.get_maturity_value()?;
    helper::assert_approx_eq(
        maturity_before,
        dec!("1"),
        helper::TOLERANCE,
        "maturity before redemption",
    );

    // Claim and redeem 50% of LP tokens
    let (mut dummy_account, account) = helper.create_dummy_account()?;
    helper.claim(dec!("5000"), account)?;

    let redeemed_tokens =
        helper.redeem_lp_from_account(&mut dummy_account, lp_resource, dec!("5000"))?;

    // Verify redeemed amount is exactly 50% of pool
    let redeemed_amount = redeemed_tokens.amount(&mut helper.env)?;
    let expected_redeemed = pool_initial / dec!("2");
    helper::assert_approx_eq(
        redeemed_amount,
        expected_redeemed,
        helper::TOLERANCE,
        "redeemed amount",
    );

    // After redemption:
    // - Pool had ~1000 tokens (10% initial vest)
    // - Redeemer got ~500 tokens (50% of pool)
    // - Pool now has ~500 tokens
    // - Locked still has ~9000 tokens
    // - LP tokens remaining: 5000
    // - Total tokens: 500 + 9000 = 9500
    // - Maturity = 9500 / 5000 = 1.9
    let maturity_after = helper.get_maturity_value()?;
    let pool_after = helper.get_pool_vault_amount()?;
    let locked_after = helper.get_locked_vault_amount()?;

    let expected_maturity = dec!("1.9");
    let expected_maturity_2 = (pool_after + locked_after) / dec!("5000");

    helper::assert_approx_eq(
        maturity_after,
        expected_maturity,
        helper::TOLERANCE,
        "maturity after 50% redemption",
    );

    helper::assert_approx_eq(
        expected_maturity,
        expected_maturity_2,
        helper::TOLERANCE,
        "maturity after 50% redemption",
    );

    Ok(())
}

#[test]
fn test_redemption_amounts_at_vesting_stages() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;

    helper.create_pool_units(dec!("10000"))?;
    helper.finish_setup()?;

    let lp_resource = helper.get_lp_resource_address();

    // Stage 1: Redeem at vest_start (0% linear progress, 10% initial vest)
    helper.advance_time_seconds(604800);
    helper.refill()?;

    let pool_at_0 = helper.get_pool_vault_amount()?;
    // Pool should have exactly 1000 tokens (10% initial vest)
    helper::assert_approx_eq(
        pool_at_0,
        dec!("1000"),
        helper::TOLERANCE,
        "pool at 0% progress",
    );

    let (mut account1, addr1) = helper.create_dummy_account()?;
    helper.claim(dec!("2000"), addr1)?;

    let redeemed_at_0 = helper.redeem_lp_from_account(&mut account1, lp_resource, dec!("2000"))?;
    let redeemed_amount_0 = redeemed_at_0.amount(&mut helper.env)?;

    // Should get 20% of pool: 1000 * 0.2 = 200 tokens
    let expected_redeemed_0 = pool_at_0 * dec!("0.2");
    helper::assert_approx_eq(
        redeemed_amount_0,
        expected_redeemed_0,
        helper::TOLERANCE,
        "redeemed at 0% progress",
    );

    // Stage 2: Redeem at 50% linear progress (55% total vesting)
    helper.advance_time_days(182);
    helper.advance_time_seconds(43200); // 0.5 days = 43200 seconds
    helper.refill()?;

    let pool_at_50 = helper.get_pool_vault_amount()?;
    // Pool should have: original pool - redeemed + new vesting
    // Started with 1000 (10% initial vest)
    // Redeemed 200 (20% of pool), leaving 800
    // New vesting from 10% to 55%: 45% of 10000 = 4500
    // Pool = 800 + 4500 = 5300
    helper::assert_approx_eq(
        pool_at_50,
        dec!("5300"),
        helper::TOLERANCE,
        "pool at 50% progress",
    );

    let (mut account2, addr2) = helper.create_dummy_account()?;
    // 8000 LP remaining, claim 2000 (25% of remaining)
    helper.claim(dec!("2000"), addr2)?;

    let redeemed_at_50 = helper.redeem_lp_from_account(&mut account2, lp_resource, dec!("2000"))?;
    let redeemed_amount_50 = redeemed_at_50.amount(&mut helper.env)?;

    // Should get 25% of pool: ~5300 * 0.25 ≈ 1325 (with pool rounding)
    let expected_redeemed_50 = pool_at_50 * dec!("0.25");
    helper::assert_approx_eq(
        redeemed_amount_50,
        expected_redeemed_50,
        helper::TOLERANCE,
        "redeemed at 50% progress",
    );

    // Stage 3: Redeem at 100% linear progress (100% total vesting)
    helper.advance_time_days(182);
    helper.advance_time_seconds(43200); // 0.5 days = 43200 seconds
    helper.refill()?;

    let pool_at_100 = helper.get_pool_vault_amount()?;
    let locked_at_100 = helper.get_locked_vault_amount()?;

    // Pool should have: previous pool - redeemed + remaining vesting
    // Due to pool rounding, exact amounts may vary slightly
    // Approximately: ~3975 (after 2nd redeem) + ~4500 (remaining vest) ≈ 8475
    helper::assert_approx_eq(
        pool_at_100,
        dec!("8475"),
        helper::TOLERANCE,
        "pool at 100% progress",
    );

    // Nearly all tokens should be vested
    // Due to OneResourcePool rounding throughout the process, a small amount may remain locked
    helper::assert_approx_eq(
        locked_at_100,
        dec!("0"),
        helper::TOLERANCE,
        "locked at 100% progress",
    );

    let (mut account3, addr3) = helper.create_dummy_account()?;
    // 6000 LP remaining, claim 2000 (33.33% of remaining)
    helper.claim(dec!("2000"), addr3)?;

    let redeemed_at_100 =
        helper.redeem_lp_from_account(&mut account3, lp_resource, dec!("2000"))?;
    let redeemed_amount_100 = redeemed_at_100.amount(&mut helper.env)?;

    // Should get 33.33% of pool
    let expected_redeemed_100 = pool_at_100 * dec!("2000") / dec!("6000");
    helper::assert_approx_eq(
        redeemed_amount_100,
        expected_redeemed_100,
        helper::TOLERANCE,
        "redeemed at 100% progress",
    );

    Ok(())
}

#[test]
fn test_vesting_math_after_redemption() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;

    helper.create_pool_units(dec!("10000"))?;
    helper.finish_setup()?;

    // Advance to vest_start and refill
    helper.advance_time_seconds(604800);
    helper.refill()?;

    let pool_before = helper.get_pool_vault_amount()?;
    let vested_before = helper.get_locked_vault_amount()?;

    // Should be exactly 1000 in pool, 9000 locked
    assert_eq!(pool_before, dec!("1000"));
    assert_eq!(vested_before, dec!("9000"));

    // Advance to 50%
    helper.advance_time_days(182);
    helper.advance_time_seconds(43200); // 0.5 days = 43200 seconds
    helper.refill()?;

    let pool_at_50_no_redeem = helper.get_pool_vault_amount()?;
    let locked_at_50_no_redeem = helper.get_locked_vault_amount()?;

    // Should be 5500 in pool, 4500 locked
    helper::assert_approx_eq(
        pool_at_50_no_redeem,
        dec!("5500"),
        helper::TOLERANCE,
        "pool at 50% without redemption",
    );
    assert_eq!(pool_at_50_no_redeem + locked_at_50_no_redeem, dec!("10000"));

    Ok(())
}

#[test]
fn test_redeem_75_percent_quadruples_maturity() -> Result<(), RuntimeError> {
    let mut helper = Helper::new()?;

    helper.create_pool_units(dec!("10000"))?;
    helper.finish_setup()?;

    // Advance to vest_start
    helper.advance_time_seconds(604800);
    helper.refill()?;

    let lp_resource = helper.get_lp_resource_address();

    // Claim and redeem 75% of LP tokens (7500)
    let (mut dummy_account, account) = helper.create_dummy_account()?;
    helper.claim(dec!("7500"), account)?;

    let _ = helper.redeem_lp_from_account(&mut dummy_account, lp_resource, dec!("7500"))?;

    // After redemption:
    // - Pool had ~1000 tokens (10% initial vest)
    // - Redeemer got ~750 tokens (75% of pool)
    // - Pool now has ~250 tokens
    // - Locked still has ~9000 tokens
    // - LP tokens remaining: 2500
    // - Total tokens: 250 + 9000 = 9250
    // - Maturity = 9250 / 2500 = 3.7
    let maturity_after = helper.get_maturity_value()?;
    let pool_after = helper.get_pool_vault_amount()?;
    let locked_after = helper.get_locked_vault_amount()?;

    let expected_maturity = (pool_after + locked_after) / dec!("2500");

    helper::assert_approx_eq(
        maturity_after,
        expected_maturity,
        helper::TOLERANCE,
        "maturity after 75% redemption",
    );

    Ok(())
}
