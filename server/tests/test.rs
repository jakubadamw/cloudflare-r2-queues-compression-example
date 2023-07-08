const INPUT: &'static str = include_str!("./input.txt");

#[tokio::test]
async fn test_server() -> anyhow::Result<()> {
    let domain = std::env::var("SERVER_DOMAIN_TO_TEST")?;
    let random_id = uuid::Uuid::new_v4().to_string();
    let mut key_url: url::Url = format!("https://{domain}").parse()?;
    key_url.path_segments_mut().unwrap().push(&random_id);

    let client = reqwest::Client::new();
    let response = client
        .post(key_url.clone())
        .header("content-type", "text-plain")
        .body(INPUT)
        .send()
        .await?;
    assert_eq!(
        response.text().await?,
        format!("{random_id} uploaded, compression scheduled")
    );

    // Don't accept zstd.
    let response = again::retry_if(
        || async {
            client
                .get(key_url.clone())
                .header("accept-encoding", "identity")
                .send()
                .await?
                .error_for_status()
        },
        |err: &reqwest::Error| err.status() == Some(reqwest::StatusCode::NOT_FOUND),
    )
    .await?;
    assert_eq!(response.text().await?, INPUT);

    // Accept zstd.
    let response = client
        .get(key_url)
        .header("accept-encoding", "zstd")
        .send()
        .await?;
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok());
    assert_eq!(content_type, Some("text/plain"));
    let content_encoding = response
        .headers()
        .get("content-encoding")
        .and_then(|value| value.to_str().ok());
    match content_encoding {
        Some("zstd") => {
            let response_bytes = response.bytes().await?.to_vec();
            let decoded = zstd::stream::decode_all(response_bytes.as_slice())?;
            let decoded_string = String::from_utf8(decoded)?;
            assert_eq!(decoded_string, INPUT);
        }
        None => {
            assert_eq!(response.text().await?, INPUT);
        }
        Some(content_encoding) => {
            panic!("Unknown content encoding: {content_encoding}");
        }
    }

    Ok(())
}
