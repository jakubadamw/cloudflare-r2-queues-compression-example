#![allow(incomplete_features)]
#![feature(return_position_impl_trait_in_trait)]

use futures_util::{Future, Stream, TryStream};

pub const R2_MULTIPART_CHUNK_MIN_SIZE: usize = 5 * 1_024 * 1_024;

#[derive(strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum KeyType {
    Raw,
    CompressedZstd,
}

pub fn full_key(base_key: &str, key_type: KeyType) -> String {
    format!("{key_type}/{base_key}")
}

pub trait TryCoalesceExt: TryStream {
    fn try_coalesce<Fut, F>(
        self,
        f: F,
    ) -> impl Stream<Item = std::result::Result<Self::Ok, Self::Error>>
    where
        Fut: Future<
            Output = std::result::Result<
                std::result::Result<Self::Ok, (Self::Ok, Self::Ok)>,
                Self::Error,
            >,
        >,
        F: FnMut(Self::Ok, Self::Ok) -> Fut;
}

impl<T: TryStream + Unpin> TryCoalesceExt for T {
    fn try_coalesce<Fut, F>(
        mut self,
        mut f: F,
    ) -> impl Stream<Item = std::result::Result<Self::Ok, Self::Error>>
    where
        Fut: Future<
            Output = std::result::Result<
                std::result::Result<Self::Ok, (Self::Ok, Self::Ok)>,
                Self::Error,
            >,
        >,
        F: FnMut(Self::Ok, Self::Ok) -> Fut,
    {
        use futures_util::TryStreamExt;

        async_stream::try_stream! {
            let mut acc = self.try_next().await?;
            while let Some(item) = self.try_next().await? {
                match f(acc.take().unwrap(), item).await? {
                    Ok(new_acc) => { acc = Some(new_acc); }
                    Err((completed_acc, new_acc)) => {
                        yield completed_acc;
                        acc = Some(new_acc);
                    }
                }
            }

            if let Some(remaining_acc) = acc.take() {
                yield remaining_acc;
            }
        }
    }
}
