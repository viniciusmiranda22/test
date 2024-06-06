mod utils;

use std::cell::RefCell;
use std::str::FromStr;
use candid::CandidType;
use sha2::Digest;
use ic_cdk::{api::management_canister::{http_request::{TransformArgs, HttpResponse}}};
use ic_cdk_macros::{query, update};
use ic_web3::{ic::{get_eth_addr, KeyInfo, ic_raw_sign}, types::{Address, TransactionParameters, U256, SignedTransaction, U64}, Web3, transports::ICHttp, contract::{tokens::Tokenize, Options}};
use utils::{generate_web3_client, KN_IN_LOCAL, default_derivation_key, get_public_key, CHAIN_ID, pubkey_to_address, generate_contract_client, KN_IN_PROD_FOR_TEST, KN_IN_PROD};

pub const ERC20_ABI: &[u8] = include_bytes!("../../abi/erc20.json");

thread_local! {
    static KEY_NAME: RefCell<String>  = RefCell::new(String::from(KN_IN_LOCAL));
}

#[query]
fn transform(response: TransformArgs) -> HttpResponse {
    let res = response.response;
    HttpResponse { status: res.status, headers: Vec::default(), body: res.body }
}

#[derive(CandidType)]
pub struct AccountInfo {
    pub address: String
}

#[update]
async fn get_ethereum_address() -> Result<AccountInfo, String> {
    let res = get_ecdsa_public_key().await;
    if let Err(msg) = res { return Err(msg) };
    let pub_key = res.unwrap();

    let res = pubkey_to_address(&pub_key);
    if let Err(msg) = res { return Err(msg) };
    let addr = res.unwrap();

    return Ok(AccountInfo {
        address: format!("0x{}", hex::encode(addr))
    })
}

#[update]
async fn get_transaction_count(
    target_addr: Option<String>
) -> Result<String, String> {
    let w3 = generate_web3_client(
        Some(300),
        None,
    )
        .map_err(|e| format!("generate_web3_client failed: {}", e))?;
    
    let canister_addr = if target_addr.is_some() {
        Address::from_str(&target_addr.unwrap()).unwrap()
    } else {
        get_eth_addr(None, None, ecdsa_key_name()).await
            .map_err(|e| format!("get_eth_addr failed: {}", e))?
    };

    let result = w3.eth()
        .transaction_count(canister_addr, None)
        .await
        .map_err(|e| format!("get tx count error: {}", e))?;
    Ok(result.to_string())
}

#[update]
async fn get_gas_price() -> Result<String, String> {
    let w3 = generate_web3_client(
        Some(300),
        None,
    )
        .map_err(|e| format!("generate_web3_client failed: {}", e))?;

    let gas_price = w3.eth()
        .gas_price()
        .await
        .map_err(|e| format!("get gas_price error: {}", e))?;
    Ok(gas_price.to_string())
}

#[update]
async fn get_ecdsa_public_key() -> Result<Vec<u8>, String> {
    get_public_key(
        None,
        vec![default_derivation_key()],
        ecdsa_key_name()
    ).await
}

#[update]
async fn sign_message(
    message: String,
) -> Result<Vec<u8>, String> {
    let message_hash = sha2::Sha256::digest(message).to_vec();
    ic_raw_sign(
        message_hash,
        vec![default_derivation_key()],
        ecdsa_key_name()
    ).await
}

#[update]
async fn balance_of_native() -> Result<String, String> {
    let w3 = generate_web3_client(Some(300), None)
        .map_err(|e| format!("generate_web3_client failed: {}", e))?;
    let canister_addr = get_eth_addr(None, None, ecdsa_key_name()).await
        .map_err(|e| format!("get_eth_addr failed: {}", e))?;
    let balance = w3
        .eth()
        .balance(canister_addr, None)
        .await
        .map_err(|e| format!("get balance failed: {}", e))?;
    Ok(balance.to_string())
}

#[update]
async fn sign_transfer_native(to: String, value: u64, tx_count: Option<u128>, gas_price: Option<u128>, max_resp: u64) -> Result<String, String> {
    let w3 = generate_web3_client(
        Some(max_resp),
        None,
    )
        .map_err(|e| format!("generate_web3_client failed: {}", e))?;
    let signed_tx = sign_transfer_native_internal(w3.clone(), to, value, tx_count, gas_price).await.unwrap();
    Ok(format!("0x{}", hex::encode(signed_tx.raw_transaction.0)))
}

#[update]
async fn transfer_native(to: String, value: u64, tx_count: Option<u128>, gas_price: Option<u128>, max_resp: u64) -> Result<String, String> {
    let w3 = generate_web3_client(
        Some(max_resp),
        None,
    )
        .map_err(|e| format!("generate_web3_client failed: {}", e))?;
    let signed_tx = sign_transfer_native_internal(w3.clone(), to, value, tx_count, gas_price).await.unwrap();
    match w3.eth().send_raw_transaction(signed_tx.raw_transaction).await {
        Ok(txhash) => { Ok(format!("0x{}", hex::encode(txhash.0))) },
        Err(e) => { Err(e.to_string()) },
    }
}

async fn sign_transfer_native_internal(w3: Web3<ICHttp>, to: String, value: u64, tx_count: Option<u128>, gas_price: Option<u128>) -> Result<SignedTransaction, String> {
    let canister_addr = get_eth_addr(None, None, ecdsa_key_name()).await
        .map_err(|e| format!("get_eth_addr failed: {}", e))?;

    let tx_count = match tx_count {
        Some(v) => U256::from(v),
        None => w3.eth()
            .transaction_count(canister_addr, None)
            .await
            .map_err(|e| format!("get tx count error: {}", e))?
    };
    let gas_price = match gas_price {
        Some(v) => U256::from(v),
        None => w3.eth()
            .gas_price()
            .await
            .map_err(|e| format!("get gas_price error: {}", e))?
    };

    let tx = TransactionParameters {
        to: Some(Address::from_str(&to).unwrap()),
        nonce: Some(tx_count),
        value: U256::from(value),
        gas_price: Some(gas_price),
        gas: U256::from(21000),
        ..Default::default()
    };
    let signed_tx = w3.accounts()
        .sign_transaction(
            tx,
            hex::encode(canister_addr),
            KeyInfo { derivation_path: vec![default_derivation_key()], key_name: ecdsa_key_name() },
            CHAIN_ID
        )
        .await
        .map_err(|e| format!("sign_transaction error: {}", e))?;
    Ok(signed_tx)
}

#[update]
async fn balance_of_erc20(token_addr: String, account_addr: Option<String>, max_resp: Option<u64>) -> Result<String, String> {
    let max_resp = if max_resp.is_some() {
        max_resp
    } else {
        Some(400) // default
    };
    let w3 = generate_web3_client(max_resp, None)?;
    let contract = generate_contract_client(w3.clone(), &token_addr, ERC20_ABI)
        .map_err(|e| format!("generate_contract_client failed: {}", e))?;
    let account_addr = match account_addr {
        Some(v) => Address::from_str(&v).unwrap(),
        None => get_eth_addr(None, None, ecdsa_key_name()).await
            .map_err(|e| format!("get_eth_addr failed: {}", e))?,
    };
    let res: U256 = contract
        .query(
            "balanceOf",
            account_addr,
            None,
            Options::default(),
            None,
        )
        .await
        .map_err(|e| format!("query contract error: {}", e))?;
    Ok(res.to_string())
}

#[update]
async fn sign_transfer_erc20(token_addr: String, to_addr: String, value: u64, tx_count: Option<u128>, gas_price: Option<u128>, max_resp: u64) -> Result<String, String> {
    let w3 = generate_web3_client(Some(max_resp), None)
        .map_err(|e| format!("generate_web3_client failed: {}", e))?;
    let signed_tx = sign_transfer_erc20_internal(
        w3.clone(),
        token_addr,
        to_addr,
        value,
        tx_count,
        gas_price
    ).await?;
    Ok(format!("0x{}", hex::encode(signed_tx.raw_transaction.0)))
}

#[update]
async fn transfer_erc20(token_addr: String, to_addr: String, value: u64, tx_count: Option<u128>, gas_price: Option<u128>, max_resp: u64) -> Result<String, String> {
    let w3 = generate_web3_client(Some(max_resp), None)
        .map_err(|e| format!("generate_web3_client failed: {}", e))?;
    let signed_tx = sign_transfer_erc20_internal(
        w3.clone(),
        token_addr,
        to_addr,
        value,
        tx_count,
        gas_price
    ).await?;
    match w3.eth().send_raw_transaction(signed_tx.raw_transaction).await {
        Ok(v) => Ok(format!("0x{}", hex::encode(v))),
        Err(msg) => Err(format!("send_raw_transaction failed: {}", msg))
    }
}

async fn sign_transfer_erc20_internal(w3: Web3<ICHttp>, token_addr: String, to_addr: String, value: u64, tx_count: Option<u128>, gas_price: Option<u128>) -> Result<SignedTransaction, String> {
    let to_addr = Address::from_str(&to_addr).unwrap();
    sign_tx(
        w3,
        &token_addr,
        ERC20_ABI,
        &"transfer",
        (to_addr, value,),
        tx_count,
        gas_price
    ).await
}

async fn sign_tx(
    w3: Web3<ICHttp>,
    contract_addr: &str,
    abi: &[u8],
    func: &str,
    params: impl Tokenize,
    tx_count: Option<u128>,
    gas_price: Option<u128>,
) -> Result<SignedTransaction, String> {
    let contract = generate_contract_client(w3.clone(), contract_addr, abi)
        .map_err(|e| format!("generate_contract_client failed: {}", e))?;
    let canister_addr = get_eth_addr(None, None, ecdsa_key_name()).await
        .map_err(|e| format!("get_eth_addr failed: {}", e))?;

    let tx_count = match tx_count {
        Some(v) => U256::from(v),
        None => w3.eth()
            .transaction_count(canister_addr, None)
            .await
            .map_err(|e| format!("get tx count error: {}", e))?
    };
    let gas_price = match gas_price {
        Some(v) => U256::from(v),
        None => w3.eth()
            .gas_price()
            .await
            .map_err(|e| format!("get gas_price error: {}", e))?
    };
    let options = Options::with(|op| {
        op.nonce = Some(tx_count);
        op.gas_price = Some(gas_price);
        op.transaction_type = Some(U64::from(2)) // EIP1559_TX_ID
    });

    match contract.sign(
        func,
        params,
        options,
        hex::encode(canister_addr),
        KeyInfo { derivation_path: vec![default_derivation_key()], key_name: ecdsa_key_name() },
        CHAIN_ID
    ).await {
        Ok(v) => Ok(v),
        Err(msg) => Err(format!("sign failed: {}", msg))
    }
}

fn ecdsa_key_name() -> String {
    KEY_NAME.with(|val| val.borrow().clone())
}
#[query]
fn debug_get_ecdsa_key_name() -> String {
    ecdsa_key_name()
}
#[update]
fn debug_use_ecdsa_key_for_local() {
    KEY_NAME.with(|val| { *val.borrow_mut() = KN_IN_LOCAL.to_string() })
}
#[update]
fn debug_use_ecdsa_key_for_test() {
    KEY_NAME.with(|val| { *val.borrow_mut() = KN_IN_PROD_FOR_TEST.to_string() })
}
#[update]
fn debug_use_ecdsa_key_for_prod() {
    KEY_NAME.with(|val| { *val.borrow_mut() = KN_IN_PROD.to_string() })
}