#[cfg(test)]
use selector::select;

#[tokio::test]
async fn it_works() {
    let f1 = async { 1 };
    let f2 = async { 2 };
    let f3 = async { 3 };
    let f4 = async { 4 };
    let f5 = async { 5 };
    let f6 = async { 6 };
    let f7 = async { 7 };
    let f8 = async { 8 };
    let f9 = async { 9 };
    let f91 = async { 91 };
    let f92 = async { 92 };
    let f10 = async { Ok::<usize, String>(10) };
    let f11 = async { Ok::<usize, String>(11) };

    let res = select! {
        biased;

        // ultra shorthand
        f9.await => _,

        // shorthand
        f7.await => 1,

        // shorthand with cond
        f8.await if true => 1,

        // normal
        x = f1.await => x,

        // normal with cond
        x = f2.await if true => x,

        // multiple futures
        // question: should x be `Either`?
        x = f3.await, f4.await => x,

        // multiple futures with cond
        x = f5.await if true, f6.await if false => x,

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
    println!("res: {:?}", res);
}
