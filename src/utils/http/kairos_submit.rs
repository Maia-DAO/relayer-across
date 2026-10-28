use alloy::{
    consensus::{EthereumTxEnvelope, TxEip4844Variant},
    eips::Encodable2718,
    hex,
    network::Ethereum,
    providers::SendableTx,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;

const KAIROS_SUBMIT_URL: &str = "https://rpc.kairos-timeboost.xyz";

#[derive(Serialize)]
struct KairosSendBundleParams {
    txs: Vec<String>,
}

#[derive(Serialize)]
struct KairosSendBundleBody {
    id: u64,
    jsonrpc: String,
    method: String,
    params: Vec<KairosSendBundleParams>,
}

#[derive(Deserialize)]
pub struct KairosSendBundleResponse {
    result: String,
}

pub async fn kairos_send_tx_in_bundle(
    tx_envelope: EthereumTxEnvelope<TxEip4844Variant>,
    bribe_tx_envelope: EthereumTxEnvelope<TxEip4844Variant>,
) -> Result<String, Box<dyn Error>> {
    let tx: SendableTx<Ethereum> = SendableTx::Envelope(tx_envelope);
    let bribe_tx: SendableTx<Ethereum> = SendableTx::Envelope(bribe_tx_envelope);

    let encoded_tx = match tx {
        SendableTx::Envelope(tx) => hex::encode_prefixed(tx.encoded_2718()),
        SendableTx::Builder(_) => return Err("Incorrect tx envelope type".into()),
    };

    let encoded_bribe_tx = match bribe_tx {
        SendableTx::Envelope(tx) => hex::encode_prefixed(tx.encoded_2718()),
        SendableTx::Builder(_) => return Err("Incorrect bribe tx envelope type".into()),
    };

    kairos_send_bundle(&encoded_tx, &encoded_bribe_tx).await
}

async fn kairos_send_bundle(tx: &str, bribe_tx: &str) -> Result<String, Box<dyn Error>> {
    // Build the JSON-RPC body dynamically using serde_json
    let body = KairosSendBundleBody {
        id: 1,
        jsonrpc: "2.0".to_string(),
        method: "timeboost_sendBundle".to_string(),
        params: vec![KairosSendBundleParams {
            txs: vec![tx.to_string(), bribe_tx.to_string()],
        }],
    };

    // Prepare the HTTP headers
    let headers = reqwest::header::HeaderMap::new();
    let client = Client::new();

    // Send the request
    let response = client
        .post(KAIROS_SUBMIT_URL)
        .json(&body) // Automatically serializes the body to JSON
        .headers(headers)
        .send()
        .await?;

    // Check if the response is successful and return the body as a string
    if response.status().is_success() {
        let response_text: KairosSendBundleResponse = response.json().await?;
        Ok(response_text.result)
    } else {
        Err(format!("Request failed with status: {}", response.status()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kairos_send_bundle() {
        let response = kairos_send_bundle(&"0x123456", &"0x123456").await;
        println!("🚀 ~ fntest_kairos_send_bundle ~ response: {:#?}", response);
    }
}
