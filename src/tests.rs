use std::sync::Arc;

use super::*;

#[test]
fn it_works() {
    let result = 2 + 2;
    assert_eq!(result, 4);
}

#[tokio::test]
async fn simple() {
    let paxos = Paxos::new(3);

    let result = paxos.run_paxos("hello".into()).await;
    assert_eq!(
        result,
        Proposal {
            number: 1,
            value: "hello".into()
        }
    );
}

#[tokio::test]
async fn concurrent_run() {
    let paxos = Arc::new(Paxos::new(3));

    let proposer_num = 10;

    let result = join_all((0..proposer_num).into_iter().map(|i| {
        let value = format!("hello {}", i);
        let paxos = paxos.clone();

        async move { paxos.clone().run_paxos(value).await }
    }))
    .await;

    assert!(result.windows(2).all(|w| w[0].value == w[1].value));
}
