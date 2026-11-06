#[soroban_sdk::contractargs(name = "Args")]
#[soroban_sdk::contractclient(name = "Client")]
pub trait Contract {
    fn mint(
        env: soroban_sdk::Env,
        shares: i128,
        receiver: soroban_sdk::Address,
        from: soroban_sdk::Address,
        operator: soroban_sdk::Address,
    ) -> i128;
    fn name(env: soroban_sdk::Env) -> soroban_sdk::String;
    fn pause(env: soroban_sdk::Env, admin: soroban_sdk::Address);
    fn redeem(
        env: soroban_sdk::Env,
        shares: i128,
        receiver: soroban_sdk::Address,
        owner: soroban_sdk::Address,
        operator: soroban_sdk::Address,
    ) -> i128;
    fn symbol(env: soroban_sdk::Env) -> soroban_sdk::String;
    fn approve(
        env: soroban_sdk::Env,
        owner: soroban_sdk::Address,
        spender: soroban_sdk::Address,
        amount: i128,
        live_until_ledger: u32,
    );
    fn balance(env: soroban_sdk::Env, account: soroban_sdk::Address) -> i128;
    fn deposit(
        env: soroban_sdk::Env,
        assets: i128,
        receiver: soroban_sdk::Address,
        from: soroban_sdk::Address,
        operator: soroban_sdk::Address,
    ) -> i128;
    fn unpause(env: soroban_sdk::Env, admin: soroban_sdk::Address);
    fn upgrade(
        env: soroban_sdk::Env,
        new_wasm_hash: soroban_sdk::BytesN<32>,
        operator: soroban_sdk::Address,
    );
    fn decimals(env: soroban_sdk::Env) -> u32;
    fn max_mint(env: soroban_sdk::Env, receiver: soroban_sdk::Address) -> i128;
    fn transfer(
        env: soroban_sdk::Env,
        from: soroban_sdk::Address,
        to: soroban_sdk::MuxedAddress,
        amount: i128,
    );
    fn withdraw(
        env: soroban_sdk::Env,
        assets: i128,
        receiver: soroban_sdk::Address,
        owner: soroban_sdk::Address,
        operator: soroban_sdk::Address,
    ) -> i128;
    fn allowance(
        env: soroban_sdk::Env,
        owner: soroban_sdk::Address,
        spender: soroban_sdk::Address,
    ) -> i128;
    fn get_admin(env: soroban_sdk::Env) -> soroban_sdk::Address;
    fn is_paused(env: soroban_sdk::Env) -> bool;
    fn extend_ttl(env: soroban_sdk::Env);
    fn max_redeem(env: soroban_sdk::Env, owner: soroban_sdk::Address) -> i128;
    fn max_deposit(env: soroban_sdk::Env, receiver: soroban_sdk::Address) -> i128;
    fn query_asset(env: soroban_sdk::Env) -> soroban_sdk::Address;
    fn accept_admin(env: soroban_sdk::Env);
    fn get_operator(env: soroban_sdk::Env) -> Option<soroban_sdk::Address>;
    fn max_withdraw(env: soroban_sdk::Env, owner: soroban_sdk::Address) -> i128;
    fn preview_mint(env: soroban_sdk::Env, shares: i128) -> i128;
    fn set_operator(
        env: soroban_sdk::Env,
        admin: soroban_sdk::Address,
        new_operator: soroban_sdk::Address,
    );
    fn total_assets(env: soroban_sdk::Env) -> i128;
    fn total_supply(env: soroban_sdk::Env) -> i128;
    fn __constructor(
        env: soroban_sdk::Env,
        name: soroban_sdk::String,
        symbol: soroban_sdk::String,
        asset: soroban_sdk::Address,
        decimals_offset: u32,
        admin: soroban_sdk::Address,
    );
    fn propose_admin(
        env: soroban_sdk::Env,
        admin: soroban_sdk::Address,
        new_admin: soroban_sdk::Address,
        deadline: u64,
    );
    fn transfer_from(
        env: soroban_sdk::Env,
        spender: soroban_sdk::Address,
        from: soroban_sdk::Address,
        to: soroban_sdk::Address,
        amount: i128,
    );
    fn add_subaccount(
        env: soroban_sdk::Env,
        admin: soroban_sdk::Address,
        subaccount: soroban_sdk::Address,
        subaccount_type: SubaccountType,
    );
    fn preview_redeem(env: soroban_sdk::Env, shares: i128) -> i128;
    fn set_aum_limits(
        env: soroban_sdk::Env,
        admin: soroban_sdk::Address,
        increase_bps: u32,
        decrease_bps: u32,
    );
    fn get_subaccounts(env: soroban_sdk::Env) -> soroban_sdk::Vec<soroban_sdk::Address>;
    fn preview_deposit(env: soroban_sdk::Env, assets: i128) -> i128;
    fn preview_withdraw(env: soroban_sdk::Env, assets: i128) -> i128;
    fn convert_to_assets(env: soroban_sdk::Env, shares: i128) -> i128;
    fn convert_to_shares(env: soroban_sdk::Env, assets: i128) -> i128;
    fn get_pending_admin(env: soroban_sdk::Env) -> Option<soroban_sdk::Address>;
    fn remove_subaccount(
        env: soroban_sdk::Env,
        admin: soroban_sdk::Address,
        subaccount: soroban_sdk::Address,
    );
    fn get_deployed_assets(env: soroban_sdk::Env) -> i128;
    fn get_subaccount_type(
        env: soroban_sdk::Env,
        subaccount: soroban_sdk::Address,
    ) -> SubaccountType;
    fn cancel_admin_proposal(env: soroban_sdk::Env, admin: soroban_sdk::Address);
    fn deposit_to_subaccount(
        env: soroban_sdk::Env,
        operator: soroban_sdk::Address,
        subaccount: soroban_sdk::Address,
        amount: i128,
    );
    fn get_strategy_balances(env: soroban_sdk::Env) -> i128;
    fn set_aum_window_limits(
        env: soroban_sdk::Env,
        admin: soroban_sdk::Address,
        window_duration: u64,
        cumulative_increase_bps: u32,
        cumulative_decrease_bps: u32,
    );
    fn get_aum_decrease_limit(env: soroban_sdk::Env) -> u32;
    fn get_aum_increase_limit(env: soroban_sdk::Env) -> u32;
    fn update_deployed_assets(
        env: soroban_sdk::Env,
        operator: soroban_sdk::Address,
        amount: i128,
    );
    fn update_wallet_deployed(
        env: soroban_sdk::Env,
        operator: soroban_sdk::Address,
        subaccount: soroban_sdk::Address,
        new_tracked: i128,
    );
    fn get_aum_window_duration(env: soroban_sdk::Env) -> u64;
    fn get_wallet_net_deployed(
        env: soroban_sdk::Env,
        subaccount: soroban_sdk::Address,
    ) -> i128;
    fn get_aum_window_dec_limit(env: soroban_sdk::Env) -> u32;
    fn get_aum_window_inc_limit(env: soroban_sdk::Env) -> u32;
    fn seed_wallet_net_deployed(
        env: soroban_sdk::Env,
        admin: soroban_sdk::Address,
        subaccount: soroban_sdk::Address,
        amount: i128,
    );
    fn withdraw_from_subaccount(
        env: soroban_sdk::Env,
        operator: soroban_sdk::Address,
        subaccount: soroban_sdk::Address,
        amount: i128,
    );
    fn get_wallet_deployed_assets(env: soroban_sdk::Env) -> i128;
    fn remove_wallet_and_reconcile(
        env: soroban_sdk::Env,
        admin: soroban_sdk::Address,
        operator: soroban_sdk::Address,
        subaccount: soroban_sdk::Address,
        new_deployed_total: i128,
    );
    fn update_wallet_deployed_batch(
        env: soroban_sdk::Env,
        operator: soroban_sdk::Address,
        updates: soroban_sdk::Vec<(soroban_sdk::Address, i128)>,
    );
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Checkpoint {
    pub timestamp: u64,
    pub votes: u128,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Operation {
    pub args: soroban_sdk::Vec<soroban_sdk::Val>,
    pub function: soroban_sdk::Symbol,
    pub predecessor: soroban_sdk::BytesN<32>,
    pub salt: soroban_sdk::BytesN<32>,
    pub target: soroban_sdk::Address,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct OwnerTokensKey {
    pub index: u32,
    pub owner: soroban_sdk::Address,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct RoyaltyInfo {
    pub basis_points: u32,
    pub receiver: soroban_sdk::Address,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Metadata {
    pub base_uri: soroban_sdk::String,
    pub name: soroban_sdk::String,
    pub symbol: soroban_sdk::String,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ApprovalData {
    pub approved: soroban_sdk::Address,
    pub live_until_ledger: u32,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Document {
    pub document_hash: soroban_sdk::BytesN<32>,
    pub timestamp: u64,
    pub uri: soroban_sdk::String,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct SigningKey {
    pub public_key: soroban_sdk::Bytes,
    pub scheme: u32,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Ed25519SignatureData {
    pub public_key: soroban_sdk::BytesN<32>,
    pub signature: soroban_sdk::BytesN<64>,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Secp256k1SignatureData {
    pub public_key: soroban_sdk::BytesN<65>,
    pub recovery_id: u32,
    pub signature: soroban_sdk::BytesN<64>,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Secp256r1SignatureData {
    pub public_key: soroban_sdk::BytesN<65>,
    pub signature: soroban_sdk::BytesN<64>,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Claim {
    pub data: soroban_sdk::Bytes,
    pub issuer: soroban_sdk::Address,
    pub scheme: u32,
    pub signature: soroban_sdk::Bytes,
    pub topic: u32,
    pub uri: soroban_sdk::String,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct CountryData {
    pub country: CountryRelation,
    pub metadata: Option<soroban_sdk::Map<soroban_sdk::Symbol, soroban_sdk::String>>,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct IdentityProfile {
    pub countries: soroban_sdk::Vec<CountryData>,
    pub identity_type: IdentityType,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Metadata {
    pub decimals: u32,
    pub name: soroban_sdk::String,
    pub symbol: soroban_sdk::String,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AllowanceKey {
    pub owner: soroban_sdk::Address,
    pub spender: soroban_sdk::Address,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AllowanceData {
    pub amount: i128,
    pub live_until_ledger: u32,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum StrategyProbeFailure {
    InvokeError,
    ConvertError,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum StorageKey {
    Admin,
    Operator,
    DeployedAssets,
    Subaccounts,
    AumIncreaseLimit,
    AumDecreaseLimit,
    Paused,
    SubaccountTypes,
    PendingAdmin,
    AdminProposalExpiry,
    AumWindowDuration,
    AumCumulativeIncreaseLimit,
    AumCumulativeDecreaseLimit,
    AumWindowStart,
    AumCumulativeIncrease,
    AumCumulativeDecrease,
    AumWindowBaseDeployed,
    WalletNetDeployed,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum SubaccountType {
    Strategy,
    Wallet,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum UpgradeableStorageKey {
    Migrating,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum MerkleDistributorStorageKey {
    Root,
    Claimed(u32),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum Rounding {
    Floor,
    Ceil,
    Truncate,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum PausableStorageKey {
    Paused,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum CheckpointType {
    TotalSupply,
    Account(soroban_sdk::Address),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum VotesStorageKey {
    Delegatee(soroban_sdk::Address),
    NumCheckpoints(soroban_sdk::Address),
    DelegateCheckpoint(soroban_sdk::Address, u32),
    NumTotalSupplyCheckpoints,
    TotalSupplyCheckpoint(u32),
    VotingUnits(soroban_sdk::Address),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum OperationState {
    Unset,
    Waiting,
    Ready,
    Done,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum TimelockStorageKey {
    MinDelay,
    Timestamp(soroban_sdk::BytesN<32>),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum NFTEnumerableStorageKey {
    TotalSupply,
    OwnerTokens(OwnerTokensKey),
    OwnerTokensIndex(u32),
    GlobalTokens(u32),
    GlobalTokensIndex(u32),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum NFTConsecutiveStorageKey {
    Approval(u32),
    Owner(u32),
    OwnershipBucket(u32),
    BurnedToken(u32),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum NFTRoyaltiesStorageKey {
    DefaultRoyalty,
    TokenRoyalty(u32),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum NFTSequentialStorageKey {
    TokenIdCounter,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum NFTStorageKey {
    Owner(u32),
    Balance(soroban_sdk::Address),
    Approval(u32),
    ApprovalForAll(soroban_sdk::Address, soroban_sdk::Address),
    Metadata,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ComplianceHook {
    Transferred,
    Created,
    Destroyed,
    CanTransfer,
    CanCreate,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ComplianceDataKey {
    HookModules(ComplianceHook),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum DocumentStorageKey {
    Index(soroban_sdk::BytesN<32>),
    Bucket(u32),
    Count,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ClaimIssuerStorageKey {
    Topics(u32),
    Pairs(SigningKey),
    RevokedClaim(soroban_sdk::BytesN<32>),
    ClaimNonce(soroban_sdk::Address, u32),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ClaimsStorageKey {
    Claim(soroban_sdk::BytesN<32>),
    ClaimsByTopic(u32),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum IdentityVerifierStorageKey {
    ClaimTopicsAndIssuers,
    IdentityRegistryStorage,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ClaimTopicsAndIssuersStorageKey {
    ClaimTopics,
    TrustedIssuers,
    IssuerClaimTopics(soroban_sdk::Address),
    ClaimTopicIssuers(u32),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum IdentityType {
    Individual,
    Organization,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum IRSStorageKey {
    Identity(soroban_sdk::Address),
    IdentityProfile(soroban_sdk::Address),
    RecoveredTo(soroban_sdk::Address),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum CountryRelation {
    Individual(IndividualCountryRelation),
    Organization(OrganizationCountryRelation),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum IndividualCountryRelation {
    Residence(u32),
    Citizenship(u32),
    SourceOfFunds(u32),
    TaxResidency(u32),
    Custom(soroban_sdk::Symbol, u32),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum OrganizationCountryRelation {
    Incorporation(u32),
    OperatingJurisdiction(u32),
    TaxJurisdiction(u32),
    SourceOfFunds(u32),
    Custom(soroban_sdk::Symbol, u32),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum TokenBinderStorageKey {
    TokenBucket(u32),
    TotalCount,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum RWAStorageKey {
    AddressFrozen(soroban_sdk::Address),
    FrozenTokens(soroban_sdk::Address),
    Compliance,
    OnchainId,
    Version,
    IdentityVerifier,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum VaultStorageKey {
    AssetAddress,
    VirtualDecimalsOffset,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum CapStorageKey {
    Cap,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum AllowListStorageKey {
    Allowed(soroban_sdk::Address),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum BlockListStorageKey {
    Blocked(soroban_sdk::Address),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum SACAdminGenericDataKey {
    Sac,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum SACAdminWrapperDataKey {
    Sac,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum FungibleStorageKey {
    Meta,
    TotalSupply,
    Balance(soroban_sdk::Address),
    Allowance(AllowanceKey),
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum VaultError {
    Unauthorized = 1,
    VaultPaused = 2,
    InvalidAmount = 3,
    SubaccountNotWhitelisted = 4,
    SubaccountAlreadyRegistered = 5,
    AumChangeExceedsLimit = 7,
    MathOverflow = 8,
    MaxSubaccountsReached = 9,
    InvalidAumLimits = 10,
    SubaccountReturnedNothing = 11,
    DeployedAssetsUnderflow = 12,
    NoPendingAdmin = 14,
    InvalidAdminProposal = 15,
    AdminProposalExpired = 16,
    AssetMismatch = 17,
    InvalidDecimalsOffset = 18,
    NegativeStrategyBalance = 19,
    AumCumulativeChangeExceedsLimit = 20,
    StrategyUnreachable = 21,
    InvalidSubaccountType = 22,
    WalletOverWithdraw = 23,
    WalletTrackerNotZero = 24,
    WalletSeedBelowTracker = 25,
    EmptyBatch = 26,
    DuplicateSubaccountInBatch = 27,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum UpgradeableError {
    MigrationNotAllowed = 1100,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum MerkleDistributorError {
    RootNotSet = 1300,
    IndexAlreadyClaimed = 1301,
    InvalidProof = 1302,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum SorobanFixedPointError {
    Overflow = 1500,
    DivisionByZero = 1501,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum CryptoError {
    MerkleProofOutOfBounds = 1400,
    MerkleIndexOutOfBounds = 1401,
    HasherEmptyState = 1402,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum PausableError {
    EnforcedPause = 1000,
    ExpectedPause = 1001,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum VotesError {
    FutureLookup = 4100,
    MathOverflow = 4101,
    InsufficientVotingUnits = 4102,
    SameDelegate = 4103,
    CheckpointNotFound = 4104,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum TimelockError {
    OperationAlreadyScheduled = 4000,
    InsufficientDelay = 4001,
    InvalidOperationState = 4002,
    UnexecutedPredecessor = 4003,
    Unauthorized = 4004,
    MinDelayNotSet = 4005,
    OperationNotScheduled = 4006,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum NonFungibleTokenError {
    NonExistentToken = 200,
    IncorrectOwner = 201,
    InsufficientApproval = 202,
    InvalidApprover = 203,
    InvalidLiveUntilLedger = 204,
    MathOverflow = 205,
    TokenIDsAreDepleted = 206,
    InvalidAmount = 207,
    TokenNotFoundInOwnerList = 208,
    TokenNotFoundInGlobalList = 209,
    UnsetMetadata = 210,
    BaseUriMaxLenExceeded = 211,
    InvalidRoyaltyAmount = 212,
    NameMaxLenExceeded = 213,
    SymbolMaxLenExceeded = 214,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ComplianceError {
    ModuleAlreadyRegistered = 360,
    ModuleNotRegistered = 361,
    ModuleBoundExceeded = 362,
    TokenNotBound = 363,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum DocumentError {
    DocumentNotFound = 380,
    MaxDocumentsReached = 381,
    UriTooLong = 382,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ClaimIssuerError {
    SigDataMismatch = 350,
    KeyIsEmpty = 351,
    KeyAlreadyAllowed = 352,
    KeyNotFound = 353,
    NotAllowed = 354,
    LimitExceeded = 355,
    NoKeysForTopic = 356,
    InvalidClaimDataExpiration = 357,
    Secp256k1RecoveryFailed = 358,
    MathOverflow = 359,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ClaimsError {
    ClaimNotFound = 340,
    ClaimNotValid = 341,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum RWAError {
    InsufficientBalance = 300,
    LessThanZero = 301,
    AddressFrozen = 302,
    InsufficientFreeTokens = 303,
    IdentityVerificationFailed = 304,
    TransferNotCompliant = 305,
    MintNotCompliant = 306,
    ComplianceNotSet = 307,
    OnchainIdNotSet = 308,
    VersionNotSet = 309,
    ClaimTopicsAndIssuersNotSet = 310,
    IdentityRegistryStorageNotSet = 311,
    IdentityVerifierNotSet = 312,
    IdentityMismatch = 313,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ClaimTopicsAndIssuersError {
    ClaimTopicDoesNotExist = 370,
    IssuerDoesNotExist = 371,
    ClaimTopicAlreadyExists = 372,
    IssuerAlreadyExists = 373,
    MaxClaimTopicsLimitReached = 374,
    MaxIssuersLimitReached = 375,
    ClaimTopicsSetCannotBeEmpty = 376,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum IRSError {
    IdentityOverwrite = 320,
    IdentityNotFound = 321,
    CountryDataNotFound = 322,
    EmptyCountryList = 323,
    MaxCountryEntriesReached = 324,
    AccountRecovered = 325,
    MetadataTooManyEntries = 326,
    MetadataStringTooLong = 327,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum TokenBinderError {
    TokenNotFound = 330,
    TokenAlreadyBound = 331,
    MaxTokensReached = 332,
    BindBatchTooLarge = 333,
    BindBatchDuplicates = 334,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum VaultTokenError {
    VaultAssetAddressNotSet = 400,
    VaultAssetAddressAlreadySet = 401,
    VaultVirtualDecimalsOffsetAlreadySet = 402,
    VaultInvalidAssetsAmount = 403,
    VaultInvalidSharesAmount = 404,
    VaultExceededMaxDeposit = 405,
    VaultExceededMaxMint = 406,
    VaultExceededMaxWithdraw = 407,
    VaultExceededMaxRedeem = 408,
    VaultMaxDecimalsOffsetExceeded = 409,
    MathOverflow = 410,
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum FungibleTokenError {
    InsufficientBalance = 100,
    InsufficientAllowance = 101,
    InvalidLiveUntilLedger = 102,
    LessThanZero = 103,
    MathOverflow = 104,
    UnsetMetadata = 105,
    ExceededCap = 106,
    InvalidCap = 107,
    CapNotSet = 108,
    SACNotSet = 109,
    SACAddressMismatch = 110,
    SACMissingFnParam = 111,
    SACInvalidFnParam = 112,
    UserNotAllowed = 113,
    UserBlocked = 114,
}
#[soroban_sdk::contractevent(export = false, topics = ["admin_set"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AdminSet {
    #[topic]
    pub admin: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["operator_set"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct OperatorSet {
    #[topic]
    pub admin: soroban_sdk::Address,
    pub old_operator: Option<soroban_sdk::Address>,
    pub new_operator: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["vault_paused"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct VaultPaused {
    #[topic]
    pub admin: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["vault_unpaused"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct VaultUnpaused {
    #[topic]
    pub admin: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["subaccount_added"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct SubaccountAdded {
    #[topic]
    pub admin: soroban_sdk::Address,
    #[topic]
    pub subaccount: soroban_sdk::Address,
    pub subaccount_type: SubaccountType,
}
#[soroban_sdk::contractevent(export = false, topics = ["aum_limits_changed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AumLimitsChanged {
    #[topic]
    pub admin: soroban_sdk::Address,
    pub old_increase_bps: u32,
    pub old_decrease_bps: u32,
    pub new_increase_bps: u32,
    pub new_decrease_bps: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["subaccount_removed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct SubaccountRemoved {
    #[topic]
    pub admin: soroban_sdk::Address,
    #[topic]
    pub subaccount: soroban_sdk::Address,
    pub total_deployed_assets: i128,
    pub strategy_balance: Option<i128>,
    pub subaccount_type: SubaccountType,
}
#[soroban_sdk::contractevent(export = false, topics = ["deposit_to_subaccount"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DepositToSubaccount {
    #[topic]
    pub operator: soroban_sdk::Address,
    #[topic]
    pub subaccount: soroban_sdk::Address,
    pub requested_amount: i128,
    pub actual_amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["admin_transfer_accepted"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AdminTransferAccepted {
    #[topic]
    pub old_admin: soroban_sdk::Address,
    #[topic]
    pub new_admin: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["admin_transfer_proposed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AdminTransferProposed {
    #[topic]
    pub admin: soroban_sdk::Address,
    pub pending_admin: soroban_sdk::Address,
    pub deadline: u64,
}
#[soroban_sdk::contractevent(export = false, topics = ["deployed_assets_changed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DeployedAssetsChanged {
    #[topic]
    pub caller: soroban_sdk::Address,
    pub old_amount: i128,
    pub new_amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["wallet_balance_diverged"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct WalletBalanceDiverged {
    #[topic]
    pub subaccount: soroban_sdk::Address,
    pub tracked_net_deployed: i128,
    pub wallet_balance: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["wallet_deployed_updated"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct WalletDeployedUpdated {
    #[topic]
    pub operator: soroban_sdk::Address,
    #[topic]
    pub subaccount: soroban_sdk::Address,
    pub old_tracked: i128,
    pub new_tracked: i128,
    pub delta: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["admin_transfer_cancelled"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AdminTransferCancelled {
    #[topic]
    pub admin: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["aum_window_limits_changed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AumWindowLimitsChanged {
    #[topic]
    pub admin: soroban_sdk::Address,
    pub old_window_duration: u64,
    pub old_cumulative_increase_bps: u32,
    pub old_cumulative_decrease_bps: u32,
    pub new_window_duration: u64,
    pub new_cumulative_increase_bps: u32,
    pub new_cumulative_decrease_bps: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["withdraw_from_subaccount"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct WithdrawFromSubaccount {
    #[topic]
    pub operator: soroban_sdk::Address,
    #[topic]
    pub subaccount: soroban_sdk::Address,
    pub requested_amount: i128,
    pub actual_amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["wallet_net_deployed_seeded"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct WalletNetDeployedSeeded {
    #[topic]
    pub admin: soroban_sdk::Address,
    #[topic]
    pub subaccount: soroban_sdk::Address,
    pub old_amount: i128,
    pub new_amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["wallet_balance_probe_failed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct WalletBalanceProbeFailed {
    #[topic]
    pub subaccount: soroban_sdk::Address,
    pub tracked_net_deployed: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["strategy_balance_probe_failed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct StrategyBalanceProbeFailed {
    #[topic]
    pub subaccount: soroban_sdk::Address,
    pub reason: StrategyProbeFailure,
}
#[soroban_sdk::contractevent(export = false, topics = ["wallet_deployed_batch_applied"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct WalletDeployedBatchApplied {
    #[topic]
    pub operator: soroban_sdk::Address,
    pub count: u32,
    pub net_delta: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["set_root"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct SetRoot {
    pub root: soroban_sdk::Bytes,
}
#[soroban_sdk::contractevent(export = false, topics = ["set_claimed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct SetClaimed {
    pub index: soroban_sdk::Val,
}
#[soroban_sdk::contractevent(export = false, topics = ["paused"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Paused {}
#[soroban_sdk::contractevent(export = false, topics = ["unpaused"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Unpaused {}
#[soroban_sdk::contractevent(export = false, topics = ["delegate_changed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DelegateChanged {
    #[topic]
    pub delegator: soroban_sdk::Address,
    pub from_delegate: Option<soroban_sdk::Address>,
    pub to_delegate: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["delegate_votes_changed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DelegateVotesChanged {
    #[topic]
    pub delegate: soroban_sdk::Address,
    pub previous_votes: u128,
    pub new_votes: u128,
}
#[soroban_sdk::contractevent(export = false, topics = ["min_delay_changed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct MinDelayChanged {
    pub old_delay: u32,
    pub new_delay: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["operation_executed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct OperationExecuted {
    #[topic]
    pub id: soroban_sdk::BytesN<32>,
    #[topic]
    pub target: soroban_sdk::Address,
    pub function: soroban_sdk::Symbol,
    pub args: soroban_sdk::Vec<soroban_sdk::Val>,
    pub predecessor: soroban_sdk::BytesN<32>,
    pub salt: soroban_sdk::BytesN<32>,
}
#[soroban_sdk::contractevent(export = false, topics = ["operation_cancelled"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct OperationCancelled {
    #[topic]
    pub id: soroban_sdk::BytesN<32>,
}
#[soroban_sdk::contractevent(export = false, topics = ["operation_scheduled"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct OperationScheduled {
    #[topic]
    pub id: soroban_sdk::BytesN<32>,
    #[topic]
    pub target: soroban_sdk::Address,
    pub function: soroban_sdk::Symbol,
    pub args: soroban_sdk::Vec<soroban_sdk::Val>,
    pub predecessor: soroban_sdk::BytesN<32>,
    pub salt: soroban_sdk::BytesN<32>,
    pub delay: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["consecutive_mint"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ConsecutiveMint {
    #[topic]
    pub to: soroban_sdk::Address,
    pub from_token_id: u32,
    pub to_token_id: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["burn"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Burn {
    #[topic]
    pub from: soroban_sdk::Address,
    pub token_id: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["set_token_royalty"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct SetTokenRoyalty {
    #[topic]
    pub receiver: soroban_sdk::Address,
    #[topic]
    pub token_id: u32,
    pub basis_points: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["set_default_royalty"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct SetDefaultRoyalty {
    #[topic]
    pub receiver: soroban_sdk::Address,
    pub basis_points: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["remove_token_royalty"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct RemoveTokenRoyalty {
    #[topic]
    pub token_id: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["mint"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Mint {
    #[topic]
    pub to: soroban_sdk::Address,
    pub token_id: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["approve"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Approve {
    #[topic]
    pub approver: soroban_sdk::Address,
    #[topic]
    pub token_id: u32,
    pub approved: soroban_sdk::Address,
    pub live_until_ledger: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["transfer"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Transfer {
    #[topic]
    pub from: soroban_sdk::Address,
    #[topic]
    pub to: soroban_sdk::Address,
    pub token_id: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["approve_for_all"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ApproveForAll {
    #[topic]
    pub owner: soroban_sdk::Address,
    pub operator: soroban_sdk::Address,
    pub live_until_ledger: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["module_added"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ModuleAdded {
    #[topic]
    pub hook: ComplianceHook,
    pub module: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["module_removed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ModuleRemoved {
    #[topic]
    pub hook: ComplianceHook,
    pub module: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["document_removed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DocumentRemoved {
    #[topic]
    pub name: soroban_sdk::BytesN<32>,
}
#[soroban_sdk::contractevent(export = false, topics = ["document_updated"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DocumentUpdated {
    #[topic]
    pub name: soroban_sdk::BytesN<32>,
    pub uri: soroban_sdk::String,
    pub document_hash: soroban_sdk::BytesN<32>,
    pub timestamp: u64,
}
#[soroban_sdk::contractevent(export = false, topics = ["key_allowed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct KeyAllowed {
    #[topic]
    pub public_key: soroban_sdk::Bytes,
    pub registry: soroban_sdk::Address,
    pub scheme: u32,
    pub claim_topic: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["key_removed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct KeyRemoved {
    #[topic]
    pub public_key: soroban_sdk::Bytes,
    pub registry: soroban_sdk::Address,
    pub scheme: u32,
    pub claim_topic: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["claim_revoked"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ClaimRevoked {
    #[topic]
    pub identity: soroban_sdk::Address,
    #[topic]
    pub claim_topic: u32,
    #[topic]
    pub revoked: bool,
    pub claim_data: soroban_sdk::Bytes,
}
#[soroban_sdk::contractevent(export = false, topics = ["signatures_invalidated"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct SignaturesInvalidated {
    #[topic]
    pub identity: soroban_sdk::Address,
    #[topic]
    pub claim_topic: u32,
    pub nonce: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["claim_added"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ClaimAdded {
    #[topic]
    pub claim: Claim,
}
#[soroban_sdk::contractevent(export = false, topics = ["claim_changed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ClaimChanged {
    #[topic]
    pub claim: Claim,
}
#[soroban_sdk::contractevent(export = false, topics = ["claim_removed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ClaimRemoved {
    #[topic]
    pub claim: Claim,
}
#[soroban_sdk::contractevent(export = false, topics = ["burn"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Burn {
    #[topic]
    pub from: soroban_sdk::Address,
    pub amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["mint"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Mint {
    #[topic]
    pub to: soroban_sdk::Address,
    pub amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["claim_topic_added"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ClaimTopicAdded {
    #[topic]
    pub claim_topic: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["claim_topic_removed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ClaimTopicRemoved {
    #[topic]
    pub claim_topic: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["trusted_issuer_added"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct TrustedIssuerAdded {
    #[topic]
    pub trusted_issuer: soroban_sdk::Address,
    pub claim_topics: soroban_sdk::Vec<u32>,
}
#[soroban_sdk::contractevent(export = false, topics = ["issuer_topics_updated"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct IssuerTopicsUpdated {
    #[topic]
    pub trusted_issuer: soroban_sdk::Address,
    pub claim_topics: soroban_sdk::Vec<u32>,
}
#[soroban_sdk::contractevent(export = false, topics = ["trusted_issuer_removed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct TrustedIssuerRemoved {
    #[topic]
    pub trusted_issuer: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["identity_stored"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct IdentityStored {
    #[topic]
    pub account: soroban_sdk::Address,
    #[topic]
    pub identity: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["country_data_added"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct CountryDataAdded {
    #[topic]
    pub account: soroban_sdk::Address,
    #[topic]
    pub country_data: CountryData,
}
#[soroban_sdk::contractevent(export = false, topics = ["identity_modified"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct IdentityModified {
    #[topic]
    pub old_identity: soroban_sdk::Address,
    #[topic]
    pub new_identity: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["identity_unstored"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct IdentityUnstored {
    #[topic]
    pub account: soroban_sdk::Address,
    #[topic]
    pub identity: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["identity_recovered"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct IdentityRecovered {
    #[topic]
    pub old_account: soroban_sdk::Address,
    #[topic]
    pub new_account: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["country_data_removed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct CountryDataRemoved {
    #[topic]
    pub account: soroban_sdk::Address,
    #[topic]
    pub country_data: CountryData,
}
#[soroban_sdk::contractevent(export = false, topics = ["country_data_modified"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct CountryDataModified {
    #[topic]
    pub account: soroban_sdk::Address,
    #[topic]
    pub country_data: CountryData,
}
#[soroban_sdk::contractevent(export = false, topics = ["tokens_frozen"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct TokensFrozen {
    #[topic]
    pub user_address: soroban_sdk::Address,
    pub amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["address_frozen"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AddressFrozen {
    #[topic]
    pub user_address: soroban_sdk::Address,
    #[topic]
    pub is_frozen: bool,
}
#[soroban_sdk::contractevent(export = false, topics = ["compliance_set"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ComplianceSet {
    #[topic]
    pub compliance: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["tokens_unfrozen"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct TokensUnfrozen {
    #[topic]
    pub user_address: soroban_sdk::Address,
    pub amount: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["recovery_success"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct RecoverySuccess {
    #[topic]
    pub old_account: soroban_sdk::Address,
    #[topic]
    pub new_account: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["identity_verifier_set"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct IdentityVerifierSet {
    #[topic]
    pub identity_verifier: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["token_onchain_id_updated"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct TokenOnchainIdUpdated {
    #[topic]
    pub onchain_id: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["claim_topics_and_issuers_set"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ClaimTopicsAndIssuersSet {
    #[topic]
    pub claim_topics_and_issuers: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["token_bound"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct TokenBound {
    #[topic]
    pub token: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["token_unbound"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct TokenUnbound {
    #[topic]
    pub token: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["deposit"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Deposit {
    #[topic]
    pub operator: soroban_sdk::Address,
    #[topic]
    pub from: soroban_sdk::Address,
    #[topic]
    pub receiver: soroban_sdk::Address,
    pub assets: i128,
    pub shares: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["withdraw"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Withdraw {
    #[topic]
    pub operator: soroban_sdk::Address,
    #[topic]
    pub receiver: soroban_sdk::Address,
    #[topic]
    pub owner: soroban_sdk::Address,
    pub assets: i128,
    pub shares: i128,
}
#[soroban_sdk::contractevent(export = false, topics = ["user_allowed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct UserAllowed {
    #[topic]
    pub user: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["user_disallowed"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct UserDisallowed {
    #[topic]
    pub user: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["user_blocked"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct UserBlocked {
    #[topic]
    pub user: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["user_unblocked"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct UserUnblocked {
    #[topic]
    pub user: soroban_sdk::Address,
}
#[soroban_sdk::contractevent(export = false, topics = ["approve"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Approve {
    #[topic]
    pub owner: soroban_sdk::Address,
    #[topic]
    pub spender: soroban_sdk::Address,
    pub amount: i128,
    pub live_until_ledger: u32,
}
#[soroban_sdk::contractevent(export = false, topics = ["transfer"])]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Transfer {
    #[topic]
    pub from: soroban_sdk::Address,
    #[topic]
    pub to: soroban_sdk::Address,
    pub to_muxed_id: Option<u64>,
    pub amount: i128,
}

