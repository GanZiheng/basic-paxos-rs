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
