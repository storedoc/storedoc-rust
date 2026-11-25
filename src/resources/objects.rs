use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use md5::{Digest, Md5};
use reqwest::multipart::{Form, Part};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Semaphore;

use crate::errors::{Result, StoreDocError};
use crate::http::HttpClient;
use crate::types::{
    ChunkedUploadOptions, ListObjectsParams, MessageResponse, ObjectVisibility, PaginatedResult,
    PresignedUploadParams, PresignedUploadResult, PresignedUrlOptions, PresignedUrlResult,
    StorageObject, UploadObjectOptions, UploadProgress,
};
use crate::validation::{
    assert_chunk_size, assert_concurrency, assert_metadata_key, assert_presign_minutes,
    assert_uuid, normalize_object_path, DEFAULT_CHUNK_SIZE, DEFAULT_CONCURRENCY,
};

#[derive(Clone)]
pub struct ObjectsResource {
    http: HttpClient,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ObjectResponse {
    Wrapped { data: StorageObject },
    Direct(StorageObject),
}

impl ObjectResponse {
    fn into_inner(self) -> StorageObject {
        match self {
            Self::Wrapped { data } => data,
            Self::Direct(obj) => obj,
        }
    }
}

#[derive(Deserialize)]
struct ChunkedInitiateResponse {
    upload_id: String,
    chunk_size: Option<usize>,
    total_chunks: Option<usize>,
}

fn visibility_str(v: &ObjectVisibility) -> &'static str {
    match v {
        ObjectVisibility::Inherit => "inherit",
        ObjectVisibility::Private => "private",
        ObjectVisibility::Public => "public",
        ObjectVisibility::Authenticated => "authenticated",
    }
}

fn filename_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("file.bin")
        .to_string()
}

impl ObjectsResource {
    pub(crate) fn new(http: HttpClient) -> Self {
        Self { http }
    }

    pub async fn list(
        &self,
        bucket_id: &str,
        params: &ListObjectsParams,
    ) -> Result<PaginatedResult<StorageObject>> {
        assert_uuid(bucket_id, "bucket_id")?;
        let mut query = Vec::new();
        if let Some(p) = params.page {
            query.push(("page".into(), p.to_string()));
        }
        if let Some(pp) = params.per_page {
            query.push(("per_page".into(), pp.to_string()));
        }
        if let Some(ref s) = params.search {
            query.push(("search".into(), s.clone()));
        }
        if let Some(ref f) = params.folder_id {
            query.push(("folder_id".into(), f.clone()));
        }
        if let Some(ref m) = params.mime_type {
            query.push(("mime_type".into(), m.clone()));
        }

        self.http
            .request_json(
                Method::GET,
                &format!("storage/buckets/{bucket_id}/objects"),
                if query.is_empty() { None } else { Some(&query) },
                None::<&()>,
            )
            .await
    }

    pub async fn get(&self, object_id: &str) -> Result<StorageObject> {
        assert_uuid(object_id, "object_id")?;
        self.http
            .request_json(
                Method::GET,
                &format!("storage/objects/{object_id}"),
                None,
                None::<&()>,
            )
            .await
    }

    pub async fn upload_bytes(
        &self,
        bucket_id: &str,
        bytes: Vec<u8>,
        options: UploadObjectOptions,
    ) -> Result<StorageObject> {
        assert_uuid(bucket_id, "bucket_id")?;

        let remote_path = normalize_object_path(options.path.as_deref().unwrap_or("file.bin"))?;
        let filename = filename_from_path(&remote_path);
        let mime = options
            .content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let mut form = Form::new()
            .text("bucket_id", bucket_id.to_string())
            .text("path", remote_path);

        if let Some(ref v) = options.visibility {
            form = form.text("visibility", visibility_str(v));
        }
        if let Some(ref fid) = options.folder_id {
            form = form.text("folder_id", fid.clone());
        }
        if let Some(ref cd) = options.content_disposition {
            form = form.text("content_disposition", cd.clone());
        }
        if let Some(ref ce) = options.content_encoding {
            form = form.text("content_encoding", ce.clone());
        }
        if let Some(ref cc) = options.cache_control {
            form = form.text("cache_control", cc.clone());
        }
        if let Some(ref meta) = options.custom_metadata {
            for (k, v) in meta {
                assert_metadata_key(k)?;
                form = form.text(format!("custom_metadata[{k}]"), v.clone());
            }
        }
        if let Some(ref tags) = options.tags {
            for tag in tags {
                if tag.len() > 128 {
                    return Err(StoreDocError::Validation {
                        message: "Invalid tag: tags must be at most 128 characters.".into(),
                        errors: None,
                    });
                }
                form = form.text("tags[]", tag.clone());
            }
        }

        let part = Part::bytes(bytes)
            .file_name(filename)
            .mime_str(&mime)
            .map_err(|e| StoreDocError::Validation {
                message: format!("Invalid content_type '{mime}': {e}"),
                errors: None,
            })?;
        form = form.part("file", part);

        let res: ObjectResponse = self.http.request_multipart("storage/upload", form).await?;
        Ok(res.into_inner())
    }

    pub async fn upload_file(
        &self,
        bucket_id: &str,
        local_path: impl AsRef<Path>,
        mut options: UploadObjectOptions,
    ) -> Result<StorageObject> {
        let path_ref = local_path.as_ref();
        if options.path.is_none() {
            let name = path_ref
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file.bin")
                .to_string();
            options.path = Some(name);
        }
        let bytes = tokio::fs::read(path_ref).await?;
        self.upload_bytes(bucket_id, bytes, options).await
    }

    pub async fn download_bytes(&self, object_id: &str) -> Result<Vec<u8>> {
        assert_uuid(object_id, "object_id")?;
        let resp = self
            .http
            .download_raw(&format!("storage/objects/{object_id}/download"))
            .await?;
        let expected_len = resp.content_length();
        let data = resp
            .bytes()
            .await
            .map_err(|e| StoreDocError::Network(e.to_string()))?;

        if let Some(expected) = expected_len {
            if data.len() as u64 != expected {
                return Err(StoreDocError::Api {
                    status: 502,
                    code: Some("SIZE_MISMATCH".into()),
                    message: format!(
                        "Incomplete download: expected {expected} bytes, received {}.",
                        data.len()
                    ),
                });
            }
        }

        Ok(data.to_vec())
    }

    /// Stream object directly to disk via an atomic `.part` temporary file.
    pub async fn download_to_file(
        &self,
        object_id: &str,
        destination: impl AsRef<Path>,
    ) -> Result<()> {
        assert_uuid(object_id, "object_id")?;
        let dest = destination.as_ref();
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        let resp = self
            .http
            .download_raw(&format!("storage/objects/{object_id}/download"))
            .await?;
        let expected_len = resp.content_length();

        let rand_suffix = uuid::Uuid::new_v4().simple().to_string();
        let temp_path = dest.with_file_name(format!(
            "{}.part-{}",
            dest.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("download"),
            &rand_suffix[..12]
        ));

        let write_result: Result<u64> = async {
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .await?;
            let mut stream = resp.bytes_stream();
            let mut written: u64 = 0;

            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(|e| StoreDocError::Network(e.to_string()))?;
                file.write_all(&bytes).await?;
                written += bytes.len() as u64;
            }
            file.flush().await?;
            Ok(written)
        }
        .await;

        match write_result {
            Ok(written) => {
                if let Some(expected) = expected_len {
                    if written != expected {
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        return Err(StoreDocError::Api {
                            status: 502,
                            code: Some("SIZE_MISMATCH".into()),
                            message: format!(
                                "Incomplete download: expected {expected} bytes, received {written}."
                            ),
                        });
                    }
                }
                if tokio::fs::rename(&temp_path, dest).await.is_err() {
                    let _ = tokio::fs::remove_file(dest).await;
                    tokio::fs::rename(&temp_path, dest).await?;
                }
                Ok(())
            }
            Err(err) => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                Err(err)
            }
        }
    }

    pub async fn delete(&self, object_id: &str) -> Result<MessageResponse> {
        assert_uuid(object_id, "object_id")?;
        self.http
            .request_json(
                Method::DELETE,
                &format!("storage/objects/{object_id}"),
                None,
                None::<&()>,
            )
            .await
    }

    pub async fn presign_url(
        &self,
        object_id: &str,
        options: PresignedUrlOptions,
    ) -> Result<PresignedUrlResult> {
        assert_uuid(object_id, "object_id")?;
        let minutes = options.minutes.unwrap_or(60);
        assert_presign_minutes(minutes)?;
        let action = options.action.unwrap_or_else(|| "download".to_string());
        let query = vec![
            ("expires_in".to_string(), minutes.to_string()),
            ("action".to_string(), action),
            (
                "restrict_ip".to_string(),
                if options.restrict_ip { "1" } else { "0" }.to_string(),
            ),
        ];

        self.http
            .request_json(
                Method::GET,
                &format!("storage/objects/{object_id}/presigned-url"),
                Some(&query),
                None::<&()>,
            )
            .await
    }

    pub async fn get_presigned_upload_url(
        &self,
        bucket_id: &str,
        params: &PresignedUploadParams,
    ) -> Result<PresignedUploadResult> {
        assert_uuid(bucket_id, "bucket_id")?;
        if params.filename.trim().is_empty() {
            return Err(StoreDocError::Validation {
                message: "Invalid filename: a non-empty filename is required.".into(),
                errors: None,
            });
        }
        if params.file_size < 1 {
            return Err(StoreDocError::Validation {
                message: "Invalid file_size: expected a positive integer.".into(),
                errors: None,
            });
        }
        let expires_in = params.expires_in.unwrap_or(60);
        if !(5..=1440).contains(&expires_in) {
            return Err(StoreDocError::Validation {
                message: "Invalid expires_in: expected an integer between 5 and 1440.".into(),
                errors: None,
            });
        }

        #[derive(Serialize)]
        struct Body<'a> {
            bucket_id: &'a str,
            filename: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            mime_type: &'a Option<String>,
            file_size: u64,
            expires_in: u32,
        }

        let mut res: PresignedUploadResult = self
            .http
            .request_json(
                Method::POST,
                "storage/upload/presigned",
                None,
                Some(&Body {
                    bucket_id,
                    filename: &params.filename,
                    mime_type: &params.mime_type,
                    file_size: params.file_size,
                    expires_in,
                }),
            )
            .await?;

        let resolved = res
            .upload_url
            .clone()
            .or_else(|| res.url.clone())
            .filter(|u| !u.is_empty())
            .ok_or_else(|| {
                StoreDocError::InvalidResponse(
                    "StoreDoc returned a presigned upload response without upload_url.".into(),
                )
            })?;

        res.upload_url = Some(resolved.clone());
        res.url = Some(resolved);
        Ok(res)
    }

    /// Chunked multipart upload for large local files with bounded concurrency,
    /// per-chunk MD5 verification, byte-ordered SHA-256 completion, and auto-abort on failure.
    pub async fn upload_chunked_file<F>(
        &self,
        bucket_id: &str,
        local_path: impl AsRef<Path>,
        options: ChunkedUploadOptions,
        on_progress: Option<F>,
    ) -> Result<StorageObject>
    where
        F: Fn(UploadProgress) + Send + Sync + 'static,
    {
        assert_uuid(bucket_id, "bucket_id")?;
        let requested_chunk_size = options.chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE);
        assert_chunk_size(requested_chunk_size)?;
        let concurrency = options.concurrency.unwrap_or(DEFAULT_CONCURRENCY);
        assert_concurrency(concurrency)?;

        let path_ref = local_path.as_ref();
        let metadata = tokio::fs::metadata(path_ref).await?;
        let total_size = metadata.len();
        if total_size < 1 {
            return Err(StoreDocError::Validation {
                message: "Chunked uploads require a non-empty file.".into(),
                errors: None,
            });
        }

        let remote_path = match options.path {
            Some(ref p) => Some(normalize_object_path(p)?),
            None => None,
        };
        let filename = remote_path
            .as_deref()
            .map(filename_from_path)
            .unwrap_or_else(|| {
                path_ref
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("upload.bin")
                    .to_string()
            });
        let mime_type = options
            .content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string());

        if let Some(ref meta) = options.custom_metadata {
            for k in meta.keys() {
                assert_metadata_key(k)?;
            }
        }
        if let Some(ref tags) = options.tags {
            for t in tags {
                if t.len() > 128 {
                    return Err(StoreDocError::Validation {
                        message: "Invalid tag: tags must be at most 128 characters.".into(),
                        errors: None,
                    });
                }
            }
        }

        #[derive(Serialize)]
        struct InitiateBody<'a> {
            bucket_id: &'a str,
            filename: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            path: &'a Option<String>,
            mime_type: &'a str,
            total_size: u64,
            chunk_size: usize,
            #[serde(skip_serializing_if = "Option::is_none")]
            folder_id: &'a Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            visibility: Option<&'static str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            content_disposition: &'a Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            content_encoding: &'a Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            cache_control: &'a Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            custom_metadata: &'a Option<std::collections::HashMap<String, String>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            tags: &'a Option<Vec<String>>,
        }

        let init: ChunkedInitiateResponse = self
            .http
            .request_json(
                Method::POST,
                "storage/upload/chunked/initiate",
                None,
                Some(&InitiateBody {
                    bucket_id,
                    filename: &filename,
                    path: &remote_path,
                    mime_type: &mime_type,
                    total_size,
                    chunk_size: requested_chunk_size,
                    folder_id: &options.folder_id,
                    visibility: options.visibility.as_ref().map(visibility_str),
                    content_disposition: &options.content_disposition,
                    content_encoding: &options.content_encoding,
                    cache_control: &options.cache_control,
                    custom_metadata: &options.custom_metadata,
                    tags: &options.tags,
                }),
            )
            .await?;

        if init.upload_id.is_empty() {
            return Err(StoreDocError::InvalidResponse(
                "StoreDoc returned a chunked-upload response without upload_id.".into(),
            ));
        }

        let chunk_size = init
            .chunk_size
            .filter(|&c| c > 0)
            .unwrap_or(requested_chunk_size);
        let total_chunks = init
            .total_chunks
            .filter(|&c| c > 0)
            .unwrap_or_else(|| (total_size as usize).div_ceil(chunk_size));

        let upload_id = Arc::new(init.upload_id);
        let path_arc = Arc::new(path_ref.to_path_buf());
        let sem = Arc::new(Semaphore::new(concurrency));
        let bytes_uploaded = Arc::new(AtomicU64::new(0));
        let chunks_received = Arc::new(AtomicUsize::new(0));
        let aborted = Arc::new(AtomicBool::new(false));
        let progress_cb = on_progress.map(Arc::new);

        let upload_result: Result<()> = async {
            let mut handles = Vec::with_capacity(total_chunks);
            for chunk_number in 0..total_chunks {
                if aborted.load(Ordering::Relaxed) {
                    break;
                }
                let permit =
                    sem.clone().acquire_owned().await.map_err(|e| {
                        StoreDocError::Network(format!("Upload semaphore closed: {e}"))
                    })?;
                if aborted.load(Ordering::Relaxed) {
                    break;
                }
                let http = self.http.clone();
                let upload_id = upload_id.clone();
                let path_arc = path_arc.clone();
                let bytes_uploaded = bytes_uploaded.clone();
                let chunks_received = chunks_received.clone();
                let aborted = aborted.clone();
                let progress_cb = progress_cb.clone();

                handles.push(tokio::spawn(async move {
                    let _permit = permit;
                    if aborted.load(Ordering::Relaxed) {
                        return Err(StoreDocError::Network("Upload aborted".into()));
                    }
                    let start = (chunk_number as u64) * (chunk_size as u64);
                    let len = ((total_size - start).min(chunk_size as u64)) as usize;

                    let mut file = tokio::fs::File::open(path_arc.as_path()).await?;
                    file.seek(std::io::SeekFrom::Start(start)).await?;
                    let mut buf = vec![0u8; len];
                    file.read_exact(&mut buf).await?;

                    let md5_hex = hex::encode(Md5::digest(&buf));
                    let part = Part::bytes(buf)
                        .file_name(format!("chunk_{chunk_number}"))
                        .mime_str("application/octet-stream")
                        .map_err(|e| StoreDocError::Network(e.to_string()))?;

                    let form = Form::new()
                        .text("upload_id", (*upload_id).clone())
                        .text("chunk_number", chunk_number.to_string())
                        .text("checksum", md5_hex)
                        .part("chunk", part);

                    let res: std::result::Result<serde_json::Value, StoreDocError> = http
                        .request_multipart("storage/upload/chunked/chunk", form)
                        .await;
                    if let Err(err) = res {
                        aborted.store(true, Ordering::Relaxed);
                        return Err(err);
                    }

                    let uploaded =
                        bytes_uploaded.fetch_add(len as u64, Ordering::Relaxed) + (len as u64);
                    let received = chunks_received.fetch_add(1, Ordering::Relaxed) + 1;

                    if let Some(ref cb) = progress_cb {
                        let percent = ((uploaded as f64 / total_size as f64) * 100.0).round() as u8;
                        cb(UploadProgress {
                            chunk_number,
                            chunks_received: received,
                            total_chunks,
                            percent: percent.min(100),
                            bytes_uploaded: uploaded,
                            total_bytes: total_size,
                        });
                    }
                    Ok::<(), StoreDocError>(())
                }));
            }

            for handle in handles {
                let res = handle
                    .await
                    .map_err(|e| StoreDocError::Network(format!("Chunk task panicked: {e}")))?;
                res?;
            }
            Ok(())
        }
        .await;

        if let Err(err) = upload_result {
            #[derive(Serialize)]
            struct AbortBody<'a> {
                upload_id: &'a str,
            }
            let _ = self
                .http
                .request_json::<serde_json::Value>(
                    Method::POST,
                    "storage/upload/chunked/abort",
                    None,
                    Some(&AbortBody {
                        upload_id: &upload_id,
                    }),
                )
                .await;
            return Err(err);
        }

        // Compute byte-ordered SHA-256 over the file
        let sha256_hex = hash_file_sha256(path_ref).await?;

        #[derive(Serialize)]
        struct CompleteBody<'a> {
            upload_id: &'a str,
            checksum: &'a str,
        }

        let complete: ObjectResponse = self
            .http
            .request_json(
                Method::POST,
                "storage/upload/chunked/complete",
                None,
                Some(&CompleteBody {
                    upload_id: &upload_id,
                    checksum: &sha256_hex,
                }),
            )
            .await?;

        Ok(complete.into_inner())
    }
}

async fn hash_file_sha256(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}
