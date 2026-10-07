//! `RelayTransport` against an in-process relay and a scripted NIP-47 wallet
//! service, so event kinds, tags, ciphers and response routing are exercised
//! end to end.

use std::net::SocketAddr;
use std::pin::Pin;

use nostr_sdk::prelude::{
    LocalRelay, LocalRelayBuilderNip42, MachineReadablePrefix, MockRelay, WritePolicy,
    WritePolicyResult,
};
use serde_json::json;

use super::*;

/// A minimal wallet service: answers `get_balance`, refuses everything else
/// with `NOT_IMPLEMENTED`, and replies in the cipher the request used. Each
/// real answer is preceded by decoys the client must not take as the answer.
async fn run_wallet_service(relay: RelayUrl, wallet: Keys, speaks_nip44: bool) -> Client {
    let client = Client::builder()
        .authenticator(SignerAuthenticator::new(wallet.clone()))
        .build();
    client.add_relay(&relay).await.unwrap();
    client.connect().and_wait(Duration::from_secs(5)).await;
    let mut info = EventBuilder::new(Kind::from_u16(INFO_KIND), "get_balance pay_invoice");
    if speaks_nip44 {
        info = info.tag(Tag::custom("encryption", ["nip44_v2 nip04"]));
    }
    client
        .send_event(&info.finalize(&wallet).unwrap())
        .await
        .unwrap();

    let mut events = client.notifications();
    client
        .subscribe(
            Filter::new()
                .kind(Kind::from_u16(REQUEST_KIND))
                .pubkey(wallet.public_key()),
        )
        .await
        .unwrap();
    let responder = client.clone();
    tokio::spawn(async move {
        while let Some(notification) = events.next().await {
            let ClientNotification::Event { event, .. } = notification else {
                continue;
            };
            if event.kind.as_u16() != REQUEST_KIND {
                continue;
            }
            let nip04 = event.content.contains("?iv=");
            let body = decrypt(&wallet, &event.pubkey, &event.content).unwrap();
            let request: Value = serde_json::from_str(&body).unwrap();
            let method = request["method"].as_str().unwrap().to_owned();
            let response = if method == "get_balance" {
                json!({"result_type": method, "result": {"balance": 21_000}})
            } else {
                json!({
                    "result_type": method,
                    "error": {"code": "NOT_IMPLEMENTED", "message": "nope"}
                })
            };
            let cipher = if nip04 { Cipher::Nip04 } else { Cipher::Nip44 };
            let reply = |signer: &Keys, request: EventId, response: &Value| {
                let sealed = cipher
                    .encrypt(signer, &event.pubkey, &response.to_string())
                    .unwrap();
                EventBuilder::new(Kind::from_u16(RESPONSE_KIND), sealed)
                    .tags([Tag::public_key(event.pubkey), Tag::event(request)])
                    .finalize(signer)
                    .unwrap()
            };
            // Answers the client must ignore, sent before the real one: one
            // from a key other than the wallet's, and one from the wallet to
            // a different request.
            let decoy = json!({"result_type": method, "result": {"balance": 1}});
            let elsewhere = EventId::from_byte_array([0; 32]);
            for fake in [
                reply(&Keys::generate(), event.id, &decoy),
                reply(&wallet, elsewhere, &decoy),
            ] {
                responder.send_event(&fake).await.unwrap();
            }
            responder
                .send_event(&reply(&wallet, event.id, &response))
                .await
                .unwrap();

            let notification = json!({
                "notification_type": "payment_received",
                "notification": {"type": "incoming", "payment_hash": "aa", "amount": 1}
            });
            let sealed = cipher
                .encrypt(&wallet, &event.pubkey, &notification.to_string())
                .unwrap();
            let kind = if nip04 {
                NIP04_NOTIFICATION_KIND
            } else {
                NIP44_NOTIFICATION_KIND
            };
            let note = EventBuilder::new(Kind::from_u16(kind), sealed)
                .tag(Tag::public_key(event.pubkey))
                .finalize(&wallet)
                .unwrap();
            responder.send_event(&note).await.unwrap();
        }
    });
    client
}

fn uri(relay: &RelayUrl, wallet: &Keys) -> NostrWalletConnectUri {
    NostrWalletConnectUri::new(
        wallet.public_key(),
        vec![relay.clone()],
        Keys::generate().secret_key().clone(),
        None,
    )
}

async fn round_trip(speaks_nip44: bool) {
    let relay = MockRelay::run().await.unwrap();
    round_trip_through(&relay, speaks_nip44).await;
}

async fn round_trip_through(relay: &LocalRelay, speaks_nip44: bool) {
    let url = relay.url().await;
    let wallet = Keys::generate();
    let _service = run_wallet_service(url.clone(), wallet.clone(), speaks_nip44).await;

    let transport = RelayTransport::connect(&uri(&url, &wallet), Duration::from_secs(5))
        .await
        .unwrap();
    let expected = if speaks_nip44 {
        Cipher::Nip44
    } else {
        Cipher::Nip04
    };
    assert_eq!(transport.cipher, expected);
    assert_eq!(
        transport.advertised_methods(),
        ["get_balance", "pay_invoice"]
    );

    let mut notifications = transport.notifications();
    let balance = transport
        .request("get_balance", json!({}), Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(balance, json!({"balance": 21_000}));
    let note = tokio::time::timeout(Duration::from_secs(5), notifications.recv())
        .await
        .expect("notification must arrive")
        .unwrap();
    assert_eq!(note.notification.payment_hash, "aa");

    match transport
        .request(
            "pay_invoice",
            json!({"invoice": "lnbc"}),
            Duration::from_secs(5),
        )
        .await
    {
        Err(TransportError::Wallet(error)) => assert_eq!(error.code, "NOT_IMPLEMENTED"),
        other => panic!("expected a wallet error, got {other:?}"),
    }
}

#[tokio::test]
async fn nip44_wallet_round_trips_requests_and_notifications() {
    round_trip(true).await;
}

#[tokio::test]
async fn legacy_nip04_wallet_round_trips_requests_and_notifications() {
    round_trip(false).await;
}

#[tokio::test]
async fn relay_that_requires_auth_is_answered() {
    let relay = LocalRelay::builder()
        .nip42(LocalRelayBuilderNip42::read_and_write())
        .build();
    relay.run().await.unwrap();
    round_trip_through(&relay, true).await;
}

/// Refuses wallet requests, as a relay that bans the kind would.
#[derive(Debug)]
struct RefuseRequests;

impl WritePolicy for RefuseRequests {
    fn admit_event<'a>(
        &'a self,
        event: &'a Event,
        _addr: &'a SocketAddr,
    ) -> Pin<Box<dyn Future<Output = WritePolicyResult> + Send + 'a>> {
        Box::pin(async move {
            if event.kind.as_u16() == REQUEST_KIND {
                WritePolicyResult::reject(MachineReadablePrefix::Blocked, "no wallet requests")
            } else {
                WritePolicyResult::Accept
            }
        })
    }
}

#[tokio::test]
async fn a_request_every_relay_refuses_reports_why_without_waiting() {
    let relay = LocalRelay::builder().write_policy(RefuseRequests).build();
    relay.run().await.unwrap();
    let url = relay.url().await;
    let wallet = Keys::generate();
    let transport = RelayTransport::connect(&uri(&url, &wallet), Duration::from_secs(2))
        .await
        .unwrap();
    let refused = tokio::time::timeout(
        Duration::from_secs(30),
        transport.request("get_balance", json!({}), Duration::from_secs(60)),
    )
    .await
    .expect("a refusal must not wait for the response timeout");
    match refused {
        // Refused is still uncertain: only an unsent request proves nothing.
        Err(TransportError::NoResponse(reason)) => {
            assert!(reason.contains("no wallet requests"), "{reason}");
        }
        other => panic!("expected an uncertain refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn unanswered_request_is_uncertain_and_an_unreachable_relay_is_unsent() {
    let relay = MockRelay::run().await.unwrap();
    let url = relay.url().await;
    // No wallet service is listening.
    let wallet = Keys::generate();
    let transport = RelayTransport::connect(&uri(&url, &wallet), Duration::from_secs(2))
        .await
        .unwrap();
    let silent = transport
        .request("get_balance", json!({}), Duration::from_millis(300))
        .await;
    assert!(
        matches!(silent, Err(TransportError::NoResponse(_))),
        "{silent:?}"
    );

    relay.shutdown();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let unsent = loop {
        let result = transport
            .request("get_balance", json!({}), Duration::from_millis(200))
            .await;
        if matches!(result, Err(TransportError::NotSent(_)))
            || tokio::time::Instant::now() > deadline
        {
            break result;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(
        matches!(unsent, Err(TransportError::NotSent(_))),
        "{unsent:?}"
    );
}
