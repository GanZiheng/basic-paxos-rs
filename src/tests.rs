use super::*;

#[test]
fn it_works() {
    let result = 2 + 2;
    assert_eq!(result, 4);
}

#[test]
fn simple() {
    let _paxos = Paxos::new(3);
}
