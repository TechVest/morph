use std::{str::FromStr, time::Duration};

use alloy::{
    network::EthereumWallet,
    primitives::Address,
    providers::{ProviderBuilder, RootProvider},
    signers::local::PrivateKeySigner,
    transports::http::{Client, Http},
};
use axum::{routing::get, Router};
use dotenv::dotenv;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming, WriteMode};
use log::Record;
use prometheus::{Encoder, TextEncoder};
use shadow_proving::{
    metrics::{METRICS, REGISTRY},
    shadow_prove::ShadowProver,
    shadow_rollup::BatchSyncer,
    util::{read_env_var, read_parse_env},
};

use tokio::time::sleep;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() {
    // Prepare environment.
    dotenv().ok();
    setup_logging();
    log::info!("Starting shadow proving...");

    // Start metric management.
    metric_mng().await;

    let l1_verify_rpc: String = read_parse_env("SHADOW_PROVING_VERIFY_L1_RPC");
    let l1_rpc: String = read_parse_env("SHADOW_PROVING_L1_RPC");
    let l2_rpc: String = read_parse_env("SHADOW_PROVING_L2_RPC");

    let private_key: String = read_parse_env("SHADOW_PROVING_PRIVATE_KEY");
    let rollup: String = read_parse_env("SHADOW_PROVING_L1_ROLLUP");
    let shadow_rollup: String = read_parse_env("SHADOW_PROVING_L1_SHADOW_ROLLUP");

    let signer: PrivateKeySigner = private_key.parse().expect("parse PrivateKeySigner");
    let wallet: EthereumWallet = EthereumWallet::from(signer.clone());
    let l1_provider: RootProvider<Http<Client>> =
        ProviderBuilder::new().on_http(l1_rpc.parse().expect("parse l1_rpc to Url"));

    let l2_provider: RootProvider<Http<Client>> =
        ProviderBuilder::new().on_http(l2_rpc.parse().expect("parse l2_rpc to Url"));

    let verify_provider: RootProvider<Http<Client>> =
        ProviderBuilder::new().on_http(l1_verify_rpc.parse().expect("parse l1_rpc to Url"));

    let l1_signer = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_provider(verify_provider.clone());

    let batch_syncer = BatchSyncer::new(
        Address::from_str(&rollup).unwrap(),
        Address::from_str(&shadow_rollup).unwrap(),
        l1_provider.clone(),
        l2_provider.clone(),
        l1_signer.clone(),
    );

    let shadow_prover = ShadowProver::new(
        signer.address(),
        Address::from_str(&shadow_rollup).unwrap(),
        verify_provider,
        l1_signer,
    );

    loop {
        sleep(Duration::from_secs(12)).await;
        // Sync & Prove
        let result = match batch_syncer.sync_batch().await {
            Ok(Some(batch)) => shadow_prover.prove(batch).await,
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        };

        // Handle result.
        match result {
            Ok(()) => (),
            Err(e) => {
                log::error!("shadow proving exec error: {:#?}", e);
            }
        }
    }
}

// Metric management
async fn metric_mng() {
    register_metrics();
    let metric_address = read_env_var("SHADOW_PROVING_METRIC_ADDRESS", "0.0.0.0:6060".to_string());
    tokio::spawn(async move {
        let metrics =
            Router::new().route("/metrics", get(handle_metrics)).layer(TraceLayer::new_for_http());
        axum::Server::bind(&metric_address.parse().unwrap())
            .serve(metrics.into_make_service())
            .await
            .unwrap();
    });
}

fn register_metrics() {
    // detected batch index.
    REGISTRY.register(Box::new(METRICS.shadow_batch_index.clone())).unwrap();
    // chunks len.
    REGISTRY.register(Box::new(METRICS.shadow_blocks_len.clone())).unwrap();
    // txn len.
    REGISTRY.register(Box::new(METRICS.shadow_txn_len.clone())).unwrap();
    // prover status.
    REGISTRY.register(Box::new(METRICS.shadow_verify_result.clone())).unwrap();
    // wallet balance.
    REGISTRY.register(Box::new(METRICS.shadow_wallet_balance.clone())).unwrap();
}

async fn handle_metrics() -> String {
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();

    // Gather the metrics.
    let mut metric_families = REGISTRY.gather();
    metric_families.extend(prometheus::gather());

    // Encode metrics to send.
    match encoder.encode(&metric_families, &mut buffer) {
        Ok(()) => String::from_utf8(buffer.clone()).unwrap(),
        Err(e) => {
            log::error!("encode metrics error: {:#?}", e);
            String::from("")
        }
    }
}

// Constants for configuration
const LOG_LEVEL: &str = "info";
const LOG_FILE_BASENAME: &str = "app_info";
const LOG_FILE_SIZE_LIMIT: u64 = 200 * 10u64.pow(6); // 200MB
                                                     // const LOG_FILE_SIZE_LIMIT: u64 = 10u64.pow(3); // 1kB
const LOG_FILES_TO_KEEP: usize = 3;

fn setup_logging() {
    //configure the logger
    Logger::try_with_env_or_str(LOG_LEVEL)
        .unwrap()
        .log_to_file(
            FileSpec::default()
                .directory(read_env_var(
                    "SHADOW_PROVING_LOG_DIR",
                    String::from("/data/logs/morph-shadow-proving"),
                ))
                .basename(LOG_FILE_BASENAME),
        )
        .format(log_format)
        .duplicate_to_stdout(Duplicate::All)
        .rotate(
            Criterion::Size(LOG_FILE_SIZE_LIMIT), // Scroll when file size reaches 200MB
            Naming::TimestampsCustomFormat {
                current_infix: Some(""),
                format: "r%Y-%m-%d_%H-%M-%S",
            }, // Using timestamps as part of scrolling files
            Cleanup::KeepLogFiles(LOG_FILES_TO_KEEP), // Keep the latest 3 scrolling files
        )
        .write_mode(WriteMode::BufferAndFlush)
        .start()
        .unwrap();
}

fn log_format(
    w: &mut dyn std::io::Write,
    now: &mut flexi_logger::DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    write!(
        w,
        "{} [{}] {} - {}",
        now.now().format("%Y-%m-%d %H:%M:%S"), // Custom time format
        record.level(),
        record.target(),
        record.args()
    )
}

#[tokio::test]
async fn test_prove_batch() {
    use alloy::{
        network::EthereumWallet,
        primitives::{Address, B256},
        providers::{ProviderBuilder, RootProvider},
        signers::local::PrivateKeySigner,
        transports::http::{Client, Http},
    };
    use shadow_proving::{abi::ShadowRollup, BatchInfo};
    use std::{env::var, str::FromStr};

    dotenv().ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let l1_rpc: String = read_parse_env("SHADOW_PROVING_L1_RPC");
    let l1_verify_rpc: String = read_parse_env("SHADOW_PROVING_VERIFY_L1_RPC");
    let private_key: String = read_parse_env("SHADOW_PROVING_PRIVATE_KEY");
    let next_tx_hash: String = read_parse_env("NEXT_TX_HASH");
    let batch_index: u64 = read_parse_env("BATCH_INDEX");

    let signer: PrivateKeySigner = private_key.parse().unwrap();
    let wallet: EthereumWallet = EthereumWallet::from(signer.clone());
    let provider: RootProvider<Http<Client>> =
        ProviderBuilder::new().on_http(l1_rpc.parse().unwrap());

    let verify_provider: RootProvider<Http<Client>> =
        ProviderBuilder::new().on_http(l1_verify_rpc.parse().unwrap());

    let shadow_rollup =
        var("SHADOW_PROVING_L1_SHADOW_ROLLUP").expect("Cannot detect L1_SHADOW_ROLLUP env var");

    let l1_signer = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(l1_verify_rpc.parse().unwrap());

    let l1_shadow_rollup =
        ShadowRollup::new(Address::from_str(&shadow_rollup).unwrap(), l1_signer.clone());

    let shadow_prover = ShadowProver::new(
        signer.address(),
        Address::from_str(&shadow_rollup).unwrap(),
        verify_provider.clone(),
        l1_signer,
    );

    let tx_hash = B256::from_str(&next_tx_hash).unwrap();
    let batch_header = shadow_proving::shadow_rollup::batch_header_inspect(&provider, tx_hash)
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

    let shadow_tx = l1_shadow_rollup.commitBatch(batch_index, batch_store);
    let rt = shadow_tx.send().await.unwrap();
    println!("commitBatch success: {:?}", rt.tx_hash());

    let batch_info = BatchInfo { batch_index, start_block: 1000001, end_block: 1000002 };

    shadow_prover.prove(batch_info).await.unwrap();
}
