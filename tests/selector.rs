#[cfg(test)]
use selector::select;

#[tokio::test]
async fn it_works() {
    let f91 = async { 91 };
    let f92 = async { 92 };
    let f10 = async { Ok::<usize, String>(10) };
    let f11 = async { Ok::<usize, String>(11) };

    let res = select! {
        biased;

        // ultra shorthand
        async { 1 }.await => _,

        // shorthand
        async{ 2 }.await => 2,

        // shorthand with cond
        async { 3 }.await if true => 3,

        // normal
        x = async { 4 }.await => x,

        // normal with cond
        x = async { 5 }.await if true => x,

        // multiple futures
        // question: should x be `Either`?
        x = async { 6 }.await , async { 7 }.await => x,

        // multiple futures with cond
        x = async { 8 }.await if true, async { 9 }.await if false => x,

        // shorthand match
        f10.await {
            Ok(x) => x,
            Err(_) => 0,
        },
        // shorthand match with cond
        f11.await if true {
            Ok(x) => x,
            Err(_) => 0,
        },

        // multiple futures with ultra short
        f91.await, f92.await => _,
    };
    assert_eq!(res, 1);
}
