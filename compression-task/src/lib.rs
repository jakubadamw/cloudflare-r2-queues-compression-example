#![allow(deprecated)]

use common::{full_key, KeyType, TryCoalesceExt};

use worker::*;

async fn read_compress_and_write(r2_bucket: &Bucket, base_r2_key: &str) -> Result<()> {
    use futures_util::{StreamExt, TryStreamExt};

    let r2_uncompressed_object_key = full_key(base_r2_key, KeyType::Raw);
    let r2_compressed_object_key = full_key(base_r2_key, KeyType::CompressedZstd);
    let object = r2_bucket
        .get(&r2_uncompressed_object_key)
        .execute()
        .await?
        .ok_or(Error::RustError(format!(
            "{r2_uncompressed_object_key} key does not exist"
        )))?;
    let byte_stream = object
        .body()
        .unwrap()
        .stream()?
        .map_ok(|chunk| bytes::Bytes::from(chunk))
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()));
    let multipart_upload = r2_bucket
        .create_multipart_upload(r2_compressed_object_key)
        .http_metadata(HttpMetadata {
            content_type: object.http_metadata().content_type,
            ..Default::default()
        })
        .execute()
        .await?;
    let compressed_byte_stream = dencode::FramedRead::new(
        async_compression::futures::bufread::ZstdEncoder::new(byte_stream.into_async_read()),
        dencode::BytesCodec {},
    );
    let uploaded_parts: Vec<_> = compressed_byte_stream
        .map_ok(|bytes| bytes.to_vec())
        .map_err(|err| Error::RustError(err.to_string()))
        .try_coalesce(|mut acc, mut item| async {
            Ok(if acc.len() < common::R2_MULTIPART_CHUNK_MIN_SIZE {
                acc.append(&mut item);
                Ok(acc)
            } else {
                Err((acc, item))
            })
        })
        .enumerate()
        .map(|(index, result)| result.map(|chunk| (index, chunk)))
        .and_then(|(index, chunk)| multipart_upload.upload_part(index as u16 + 1, chunk))
        .try_collect()
        .await?;
    multipart_upload
        .complete(uploaded_parts.into_iter())
        .await?;
    Ok(())
}

#[event(start)]
pub fn main() {
    console_error_panic_hook::set_once();
}

#[event(queue)]
pub async fn queue(message_batch: MessageBatch<String>, env: Env, _ctx: Context) -> Result<()> {
    use futures_util::TryFutureExt;

    let r2_bucket = env.bucket("BUCKET")?;

    futures_util::future::try_join_all(
        message_batch
            .messages()?
            .iter()
            .map(|message| read_compress_and_write(&r2_bucket, &message.body)),
    )
    .map_ok(|_| ())
    .await
}
