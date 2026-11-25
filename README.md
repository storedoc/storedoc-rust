# storedoc-rust

Official StoreDoc SDK for Rust and **Tauri** desktop/mobile applications.

## Installation (in your Tauri `src-tauri/Cargo.toml`)

```toml
[dependencies]
storedoc = { path = "../../storedoc-rust" } # or git URL
```

## Tauri Usage Example (`src-tauri/src/lib.rs`)

```rust
use std::time::Duration;
use storedoc::{
    ListObjectsParams, StorageBucket, StorageObject, StoreDocClient, StoreDocConfig,
    StoreDocError, UploadObjectOptions,
};
use tauri::State;

#[tauri::command]
async fn list_buckets(client: State<'_, StoreDocClient>) -> Result<Vec<StorageBucket>, StoreDocError> {
    client.buckets().list().await
}

#[tauri::command]
async fn list_files(
    client: State<'_, StoreDocClient>,
    bucket_id: String,
) -> Result<Vec<StorageObject>, StoreDocError> {
    let page = client
        .objects()
        .list(&bucket_id, &ListObjectsParams::default())
        .await?;
    Ok(page.data)
}

#[tauri::command]
async fn upload_local_file(
    client: State<'_, StoreDocClient>,
    bucket_id: String,
    local_path: String,
    remote_path: Option<String>,
) -> Result<StorageObject, StoreDocError> {
    client
        .objects()
        .upload_file(
            &bucket_id,
            local_path,
            UploadObjectOptions {
                path: remote_path,
                ..Default::default()
            },
        )
        .await
}

pub fn run() {
    let config = StoreDocConfig::new("https://your-storedoc-instance.com/api")
        .with_hmac("STD_AK_...", "STD_SK_...")
        .timeout(Duration::from_secs(60));

    let client = StoreDocClient::new(config).expect("valid StoreDoc config");

    tauri::Builder::default()
        .manage(client)
        .invoke_handler(tauri::generate_handler![list_buckets, list_files, upload_local_file])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```
