use crate::{metrics::METRICS, util::read_env_var, BatchInfo};
use alloy::{
    consensus::Transaction,
    network::{Network, ReceiptResponse},
    primitives::{Address, Bytes, TxHash, U256, U64},
    providers::{Provider, RootProvider},
    rpc::types::Log,
    sol_types::SolCall,
    transports::{
        http::{Client, Http},
        Transport,
    },
};

use crate::{
    Rollup::{self, RollupInstance},
    ShadowRollup::{self, ShadowRollupInstance},
};

#[derive(Clone, Debug)]
pub struct BatchSyncer<T, P, N> {
    l1_provider: RootProvider<Http<Client>>,
    l2_provider: RootProvider<Http<Client>>,
    l1_rollup: RollupInstance<Http<Client>, RootProvider<Http<Client>>>,
    l1_shadow_rollup: ShadowRollupInstance<T, P, N>,
}

impl<T, P, N> BatchSyncer<T, P, N>
where
    P: Provider<T, N> + Clone,
    T: Transport + Clone,
    N: Network,
{
    pub fn new(
        rollup_address: Address,
        shadow_rollup_address: Address,
        l1_provider: RootProvider<Http<Client>>,
        l2_provider: RootProvider<Http<Client>>,
        wallet: P,
    ) -> Self {
        let l1_rollup = Rollup::RollupInstance::new(rollup_address, l1_provider.clone());
        let l1_shadow_rollup = ShadowRollup::new(shadow_rollup_address, wallet);

        Self { l1_provider, l2_provider, l1_rollup, l1_shadow_rollup }
    }

    /**
     * Sync a latest batch to l1-shadow-rollup.
     */
    pub async fn sync_batch(&self) -> Result<Option<BatchInfo>, anyhow::Error> {
        log::info!("start sync_batch...");

        let latest = self.l1_provider.get_block_number().await?;

        // Fetch a commited batch on l1 rollup.
        let (batch_info, batch_header) = match get_committed_batch(
            U64::from(latest),
            &self.l1_rollup,
            &self.l1_provider,
            &self.l2_provider,
        )
        .await
        {
            Ok(Some(committed_batch)) => committed_batch,
            Ok(None) => return Ok(None),
            Err(msg) => {
                log::error!("get_committed_batch error: {:?}", msg);
                return Ok(None);
            }
        };

        // Batch should not have been verified yet.
        if is_prove_success(batch_info.batch_index, &self.l1_shadow_rollup).await.unwrap_or(true) {
            log::debug!("batch of {:?} already prove state successful", batch_info.batch_index);
            return Ok(None);
        };

        // Assembling a batche of the same commitment.
        #[rustfmt::skip]
        //   Below is the encoding for `BatchHeader`, reference: morph-repo/contracts/contracts/libraries/codec/BatchHeaderCodecV1.sol
        //    
        //   * Field                   Bytes       Type        Index   Comments
        //   * version                 1           uint8       0       The batch version
        //   * batchIndex              8           uint64      1       The index of the batch
        //   * l1MessagePopped         8           uint64      9       Number of L1 messages popped in the batch
        //   * totalL1MessagePopped    8           uint64      17      Number of total L1 messages popped after the batch
        //   * dataHash                32          bytes32     25      The data hash of the batch
        //   * blobVersionedHash       32          bytes32     57      The versioned hash of the blob with this batch’s data
        //   * prevStateHash           32          bytes32     89      Preview state root
        //   * postStateHash           32          bytes32     121     Post state root
        //   * withdrawRootHash        32          bytes32     153     L2 withdrawal tree root hash
        //   * sequencerSetVerifyHash  32          bytes32     185     L2 sequencers set verify hash
        //   * parentBatchHash         32          bytes32     217     The parent batch hash
        //   * skippedL1MessageBitmap  dynamic     uint256[]   249     A bitmap to indicate which L1 messages are skipped in the batch
        //   @dev Below is the feilds for `BatchHeader` V1
        //   * lastBlockNumber         8           uint64      249     The last block number in this batch
        // ```
        let batch_store = ShadowRollup::BatchStore {
            prevStateRoot: batch_header
                .get(89..121)
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            postStateRoot: batch_header
                .get(121..153)
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            withdrawalRoot: batch_header
                .get(153..185)
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            dataHash: batch_header.get(25..57).unwrap_or_default().try_into().unwrap_or_default(),
            blobVersionedHash: batch_header
                .get(57..89)
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            sequencerSetVerifyHash: batch_header
                .get(185..217)
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
        };

        log::info!(
            "sync batch of {:?}, prevStateRoot = {:?}, postStateRoot = {:?}, withdrawalRoot = {:?},
            dataHash = {:?}, blobVersionedHash = {:?}, sequencerSetVerifyHash = {:?}",
            batch_info.batch_index,
            alloy::hex::encode_prefixed(batch_store.prevStateRoot),
            alloy::hex::encode_prefixed(batch_store.postStateRoot),
            alloy::hex::encode_prefixed(batch_store.withdrawalRoot),
            alloy::hex::encode_prefixed(batch_store.dataHash),
            alloy::hex::encode_prefixed(batch_store.blobVersionedHash),
            alloy::hex::encode_prefixed(batch_store.sequencerSetVerifyHash),
        );

        // Commit the shadow batch.
        let shadow_tx = self.l1_shadow_rollup.commitBatch(batch_info.batch_index, batch_store);
        let rt = shadow_tx.send().await;
        let pending_tx = match rt {
            Ok(pending_tx) => pending_tx,
            Err(e) => {
                log::error!("send tx of shadow_rollup.commit_batch error: {:#?}", e);
                return Ok(None);
            }
        };
        let receipt = pending_tx.get_receipt().await.unwrap();
        if !receipt.status() {
            log::error!("shadow_rollup.commit_batch check_receipt fail");
            return Ok(None);
        }

        log::info!(">Sync shadow batch complete: {:#?}", batch_info.batch_index);
        Ok(Some(batch_info))
    }
}

async fn get_committed_batch<T, P, N>(
    latest: U64,
    l1_rollup: &RollupInstance<T, P, N>,
    l1_provider: &RootProvider<Http<Client>>,
    l2_provider: &RootProvider<Http<Client>>,
) -> Result<Option<(BatchInfo, Bytes)>, String>
where
    P: Provider<T, N> + Clone,
    T: Transport + Clone,
    N: Network,
{
    log::info!("latest l1 blocknum = {:#?}", latest);
    let start = if latest > U64::from(600) { latest - U64::from(600) } else { U64::from(1) };
    let filter =
        l1_rollup.CommitBatch_filter().filter.from_block(start).address(*l1_rollup.address());
    let mut logs: Vec<Log> = match l1_provider.get_logs(&filter).await {
        Ok(logs) => logs,
        Err(e) => {
            log::error!("l1_rollup.commit_batch.get_logs error: {:#?}", e);
            return Err("l1_rollup.commit_batch.get_logs provider error".to_string());
        }
    };
    if logs.is_empty() {
        log::warn!("There have been no commit_batch logs for the last 600 blocks");
        return Ok(None);
    }
    if logs.len() < 3 {
        log::warn!("No enough commit_batch logs for the last 600 blocks");
        return Ok(None);
    }
    logs.sort_by(|a, b| a.block_number.unwrap().cmp(&b.block_number.unwrap()));

    let batch_index = match logs.get(logs.len() - 2) {
        Some(log) => {
            let _index = U256::from_be_slice(log.topics()[1].as_slice());
            _index.to::<u64>()
        }
        None => {
            return Err("find commit_batch log error".to_string());
        }
    };

    if batch_index == 0 {
        return Err(String::from("batch_index is 0"));
    }
    let (blocks, total_txn_count) =
        match batch_blocks_inspect(l1_rollup, l2_provider, batch_index).await {
            Some(block_txn) => block_txn,
            None => return Err(String::from("batch_blocks_inspect none")),
        };

    if blocks.0 <= blocks.1 {
        return Err(String::from("blocks is empty"));
    }

    if blocks.1 - blocks.0 + 1 > read_env_var("SHADOW_PROVING_MAX_BLOCK", 300) {
        log::warn!("Too many blocks in the latest batch to shadow prove");
        return Ok(None);
    }

    if total_txn_count > read_env_var("SHADOW_PROVING_MAX_TXN", 600) {
        log::warn!("Too many txn in the latest batch to shadow prove");
        return Ok(None);
    }

    let batch_info: BatchInfo =
        BatchInfo { batch_index, start_block: blocks.0, end_block: blocks.1 };

    // A rollup commit_batch_input contains prev batch_header.
    let next_tx_hash = match logs.last() {
        Some(log) => log.transaction_hash.unwrap_or_default(),

        None => {
            return Err("find commit_batch log error".to_string());
        }
    };
    let batch_header = batch_header_inspect(l1_provider, next_tx_hash)
        .await
        .ok_or_else(|| "Failed to inspect batch header".to_string())?;

    log::info!("Found the committed batch, batch index = {:#?}", batch_index);
    Ok(Some((batch_info, batch_header)))
}

pub async fn batch_header_inspect(
    l1_provider: &RootProvider<Http<Client>>,
    hash: TxHash,
) -> Option<Bytes> {
    //Step1.  Get transaction
    let result = l1_provider.get_transaction_by_hash(hash).await;
    let tx = match result {
        Ok(Some(tx)) => tx,
        Ok(None) => {
            log::error!("l1_provider.get_transaction is none");
            return None;
        }
        Err(e) => {
            log::error!("l1_provider.get_transaction err: {:#?}", e);
            return None;
        }
    };

    //Step2. Parse transaction data
    let data = tx.input();

    if data.is_empty() {
        log::warn!("batch inspect: tx.input is empty, tx_hash =  {:#?}", hash);
        return None;
    }
    let param = if let Ok(_param) = Rollup::commitBatchCall::abi_decode(&data, false) {
        _param
    } else {
        log::error!("batch inspect: decode tx.input error, tx_hash =  {:#?}", hash);
        return None;
    };
    let parent_batch_header: Bytes = param.batchDataInput.parentBatchHeader;
    Some(parent_batch_header)
}

async fn batch_blocks_inspect<T, P, N>(
    l1_rollup: &RollupInstance<T, P, N>,
    l2_provider: &RootProvider<Http<Client>>,
    batch_index: u64,
) -> Option<((u64, u64), u64)>
where
    P: Provider<T, N> + Clone,
    T: Transport + Clone,
    N: Network,
{
    let prev_bn = match l1_rollup.batchDataStore(U256::from(batch_index - 1)).call().await {
        Ok(s) => s.blockNumber.to::<u64>(),
        Err(e) => {
            log::error!("l1_rollup.batch_data_store err: {:#?}", e);
            return None;
        }
    };

    let current_bn = match l1_rollup.batchDataStore(U256::from(batch_index)).call().await {
        Ok(s) => s.blockNumber.to::<u64>(),
        Err(e) => {
            log::error!("l1_rollup.batch_data_store err: {:#?}", e);
            return None;
        }
    };

    let mut total_tx_count: u64 = 0;
    for i in prev_bn + 1..current_bn + 1 {
        total_tx_count += l2_provider
            .get_block_transaction_count_by_number(i.into())
            .await
            .unwrap_or_default()
            .unwrap_or_default();
    }

    log::info!(
        "decode_blocks, blocks_len: {:#?}, start_block: {:#?}, txn_in_batch: {:?}",
        current_bn - prev_bn,
        prev_bn + 1,
        total_tx_count
    );

    METRICS.shadow_txn_len.set(total_tx_count as i64);

    Some(((prev_bn + 1, current_bn), total_tx_count))
}

async fn is_prove_success<T, P, N>(
    batch_index: u64,
    l1_rollup: &ShadowRollupInstance<T, P, N>,
) -> Option<bool>
where
    P: Provider<T, N> + Clone,
    T: Transport + Clone,
    N: Network,
{
    let is_prove_success: bool =
        match l1_rollup.isProveSuccess(U256::from(batch_index)).call().await {
            Ok(x) => x._0,
            Err(e) => {
                log::info!(
                    "query l1_shadow_rollup.is_prove_success error, batch index = {:#?}, {:#?}",
                    batch_index,
                    e
                );
                return None;
            }
        };
    Some(is_prove_success)
}

#[tokio::test]
async fn test_sync_batch() {
    use alloy::{
        network::EthereumWallet,
        primitives::Address,
        providers::{ProviderBuilder, RootProvider},
        signers::local::PrivateKeySigner,
        transports::http::{Client, Http},
    };
    use std::{env::var, str::FromStr};

    let l1_rpc: String = var("SHADOW_PROVING_VERIFY_L1_RPC").unwrap_or(
        var("SHADOW_PROVING_L1_RPC").expect("Shadow prove cannot detect L1_RPC env var"),
    );
    let l2_rpc: String = var("SHADOW_PROVING_VERIFY_L2_RPC").unwrap_or(
        var("SHADOW_PROVING_L2_RPC").expect("Shadow prove cannot detect L2_RPC env var"),
    );
    let private_key = var("SHADOW_PROVING_PRIVATE_KEY")
        .expect("Cannot detect SHADOW_PROVING_PRIVATE_KEY env var");

    let signer: PrivateKeySigner = private_key.parse().unwrap();
    let wallet: EthereumWallet = EthereumWallet::from(signer.clone());
    let l1_provider: RootProvider<Http<Client>> =
        ProviderBuilder::new().on_http(l1_rpc.parse().unwrap());
    let l2_provider: RootProvider<Http<Client>> =
        ProviderBuilder::new().on_http(l2_rpc.parse().unwrap());

    let rollup = var("SHADOW_PROVING_L1_ROLLUP").expect("Cannot detect L1_ROLLUP env var");
    let shadow_rollup =
        var("SHADOW_PROVING_L1_SHADOW_ROLLUP").expect("Cannot detect L1_SHADOW_ROLLUP env var");

    let l1_signer = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(l1_rpc.parse().unwrap());

    let bs = BatchSyncer::new(
        Address::from_str(&rollup).unwrap(),
        Address::from_str(&shadow_rollup).unwrap(),
        l1_provider,
        l2_provider,
        l1_signer,
    );
    bs.sync_batch().await.unwrap();
}

#[tokio::test]
async fn test_inspect_batch_header() {
    use alloy::{primitives::B256, providers::ProviderBuilder};
    use std::str::FromStr;

    let provider: RootProvider<Http<Client>> = ProviderBuilder::new().on_http(
        "https://eth-holesky.g.alchemy.com/v2/xxxxxxx".parse().expect("parse l1_rpc to Url"),
    );
    let next_tx_hash =
        B256::from_str("0x2bdfb2bd0b8c9210bfb593cc5734e3f092fcdd54fe74c46a938448b0422089f7")
            .unwrap();
    let batch_header = batch_header_inspect(&provider, next_tx_hash)
        .await
        .ok_or_else(|| "Failed to inspect batch header".to_string())
        .unwrap();

    let batch_store = ShadowRollup::BatchStore {
        prevStateRoot: batch_header.get(89..121).unwrap_or_default().try_into().unwrap_or_default(),
        postStateRoot: batch_header
            .get(121..153)
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default(),
        withdrawalRoot: batch_header
            .get(153..185)
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default(),
        dataHash: batch_header.get(25..57).unwrap_or_default().try_into().unwrap_or_default(),
        blobVersionedHash: batch_header
            .get(57..89)
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default(),
        sequencerSetVerifyHash: batch_header
            .get(185..217)
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default(),
    };

    println!(
        "sync batch of {:?}, prevStateRoot = {:?}, postStateRoot = {:?}, withdrawalRoot = {:?},
            dataHash = {:?}, blobVersionedHash = {:?}, sequencerSetVerifyHash = {:?}",
        "batch_info.batch_index",
        hex::encode(batch_store.prevStateRoot.as_slice()),
        hex::encode(batch_store.postStateRoot.as_slice()),
        hex::encode(batch_store.withdrawalRoot.as_slice()),
        hex::encode(batch_store.dataHash.as_slice()),
        hex::encode(batch_store.blobVersionedHash.as_slice()),
        hex::encode(batch_store.sequencerSetVerifyHash.as_slice()),
    );
    // prevStateRoot =
    // "13a862a764f09e1300ad485fadcc130741d400e8d5be3dbb968901e6590e25ca", postStateRoot =
    // "20a6aa14638839f76d2b233499439e45cd315434f9628902793c421ec71fcb0c", withdrawalRoot =
    // "eda0cccc67b86712eea4536d186be3d412b86c4c56741d641d1bbfdd26b5d40b",         dataHash =
    // "89a1c4692d97c7a4a516b35bc46963da3425af5273cb5a7b8ee2cdcf41c6fa65", blobVersionedHash =
    // "013f8fabf23fba03c52572d3403d175d952937cdd78bb8e9e06eb6ffa751fd2a", sequencerSetVerifyHash =
    // "60f10881edf25485d6d9db1c3a634c002bf4da64cce0f9a0f528e00f1ead3dec"
}
