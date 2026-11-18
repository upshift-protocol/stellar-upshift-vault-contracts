#[soroban_sdk::contractargs(name = "Args")]
#[soroban_sdk::contractclient(name = "Client")]
pub trait Contract {
    fn deposit(env: soroban_sdk::Env, from: soroban_sdk::Address, amount: i128);
    fn withdraw(env: soroban_sdk::Env, to: soroban_sdk::Address, amount: i128) -> i128;
    fn get_asset(env: soroban_sdk::Env) -> soroban_sdk::Address;
    fn get_vault(env: soroban_sdk::Env) -> soroban_sdk::Address;
    fn extend_ttl(env: soroban_sdk::Env);
    fn get_balance(env: soroban_sdk::Env) -> i128;
    fn __constructor(
        env: soroban_sdk::Env,
        asset: soroban_sdk::Address,
        vault: soroban_sdk::Address,
        controller: soroban_sdk::Address,
    );
    fn get_controller(env: soroban_sdk::Env) -> soroban_sdk::Address;
    fn set_controller(
        env: soroban_sdk::Env,
        current: soroban_sdk::Address,
        new_controller: soroban_sdk::Address,
    );
    fn recover_donation(
        env: soroban_sdk::Env,
        controller: soroban_sdk::Address,
        recipient: soroban_sdk::Address,
        amount: i128,
    );
    fn get_local_balance(env: soroban_sdk::Env) -> i128;
    fn deploy_to_protocol(
        env: soroban_sdk::Env,
        controller: soroban_sdk::Address,
        protocol: soroban_sdk::Address,
        amount: i128,
    );
    fn get_deployed_total(env: soroban_sdk::Env) -> i128;
    fn seed_local_balance(
        env: soroban_sdk::Env,
        controller: soroban_sdk::Address,
        amount: i128,
    );
    fn seed_deployed_total(
        env: soroban_sdk::Env,
        controller: soroban_sdk::Address,
        amount: i128,
    );
    fn recall_from_protocol(
        env: soroban_sdk::Env,
        controller: soroban_sdk::Address,
        protocol: soroban_sdk::Address,
        amount: i128,
    );
    fn settle_protocol_returns(
        env: soroban_sdk::Env,
        controller: soroban_sdk::Address,
        new_total: i128,
    );
    fn is_deployed_total_initialized(env: soroban_sdk::Env) -> bool;
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum StrategyError {
    NotVault = 1,
    NotController = 2,
    InvalidAmount = 3,
    InsufficientBalance = 4,
    InvalidTarget = 5,
    InvalidController = 6,
    MathOverflow = 7,
    BalanceDecreasedOnTransfer = 8,
    SettleRateLimitExceeded = 9,
    NoDonationToRecover = 10,
    LocalBalanceAlreadySeeded = 11,
    SeedExceedsActualBalance = 12,
    DeployedTotalAlreadyInitialized = 13,
    DepositShortfall = 14,
}
#[soroban_sdk::contractevent(export = false, topics = ["donation_detected"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DonationDetected {
    pub excess: i128,
    pub tracked: i128,
    pub actual: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["controller_changed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ControllerChanged {
    #[topic]
    pub old_controller: soroban_sdk::Address,
    pub new_controller: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["donation_recovered"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DonationRecovered {
    #[topic]
    pub controller: soroban_sdk::Address,
    #[topic]
    pub recipient: soroban_sdk::Address,
    pub amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["deployed_to_protocol"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DeployedToProtocol {
    #[topic]
    pub controller: soroban_sdk::Address,
    #[topic]
    pub protocol: soroban_sdk::Address,
    pub amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["local_balance_seeded"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct LocalBalanceSeeded {
    #[topic]
    pub controller: soroban_sdk::Address,
    pub amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["deployed_total_seeded"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DeployedTotalSeeded {
    #[topic]
    pub controller: soroban_sdk::Address,
    pub amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["deployed_total_settled"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DeployedTotalSettled {
    #[topic]
    pub controller: soroban_sdk::Address,
    pub previous: i128,
    pub new_total: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["recalled_from_protocol"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct RecalledFromProtocol {
    #[topic]
    pub controller: soroban_sdk::Address,
    #[topic]
    pub protocol: soroban_sdk::Address,
    pub amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["tracker_exceeds_balance"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct TrackerExceedsBalance {
    pub tracked: i128,
    pub actual: i128,
    pub shortfall: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["deployed_total_underflow"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DeployedTotalUnderflow {
    pub tracked: i128,
    pub actual_recall: i128,
}

