use super::*;

#[test]
fn should_fold_duplicate_notification_pairs_within_one_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("notify_deduplication_key").await;
        let mut listener_config = tokio_postgres::Config::new();
        listener_config
            .host("127.0.0.1")
            .port(server.addr.port())
            .user("root")
            .password("postgres")
            .dbname("postgres");
        let (listener, mut listener_connection) = listener_config
            .connect(NoTls)
            .await
            .expect("connect the notification listener");
        let (notification_sender, mut notifications) = tokio::sync::mpsc::unbounded_channel();
        let notification_task = tokio::spawn(async move {
            loop {
                match std::future::poll_fn(|context| listener_connection.poll_message(context))
                    .await
                {
                    Some(Ok(tokio_postgres::AsyncMessage::Notification(notification))) => {
                        if notification_sender
                            .send((
                                notification.channel().to_string(),
                                notification.payload().to_string(),
                            ))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => return,
                }
            }
        });
        listener
            .batch_execute("LISTEN compat_dedup_channel_a; LISTEN compat_dedup_channel_b")
            .await
            .expect("subscribe to both notification channels");
        let (publisher, publisher_connection) = server.connect().await;

        // Act
        publisher
            .batch_execute(
                "BEGIN; \
                 NOTIFY compat_dedup_channel_a, 'same'; \
                 NOTIFY compat_dedup_channel_a, 'different'; \
                 NOTIFY compat_dedup_channel_a, 'same'; \
                 NOTIFY compat_dedup_channel_b, 'same'; \
                 NOTIFY compat_dedup_channel_b, 'same'; \
                 COMMIT",
            )
            .await
            .expect("commit notifications with duplicate and distinct keys");
        let first = tokio::time::timeout(Duration::from_secs(2), notifications.recv())
            .await
            .expect("first committed notification should arrive")
            .expect("notification receiver should remain open");
        let second = tokio::time::timeout(Duration::from_secs(2), notifications.recv())
            .await
            .expect("second committed notification should arrive")
            .expect("notification receiver should remain open");
        let third = tokio::time::timeout(Duration::from_secs(2), notifications.recv())
            .await
            .expect("third committed notification should arrive")
            .expect("notification receiver should remain open");
        let extra = tokio::time::timeout(Duration::from_millis(250), notifications.recv()).await;

        // Assert
        notification_task.abort();
        drop((listener, publisher));
        server.shutdown(publisher_connection).await;
        let _ = notification_task.await;

        assert_eq!(
            [first, second, third],
            [
                ("compat_dedup_channel_a".to_string(), "same".to_string()),
                (
                    "compat_dedup_channel_a".to_string(),
                    "different".to_string()
                ),
                ("compat_dedup_channel_b".to_string(), "same".to_string()),
            ],
            "only repeated channel-and-payload pairs are folded, in first-occurrence order"
        );
        assert!(
            extra.is_err(),
            "duplicate channel-and-payload pairs must not produce extra deliveries"
        );
    });
}
