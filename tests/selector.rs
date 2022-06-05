#[cfg(test)]
use selector::select;

#[tokio::test]
async fn it_works() {
    let f1 = async { 1 };
    let f2 = async { 2 };

    let res = select! {
        biased;
        // ultra shorthand
        // f1.await => _,

        // normal
        x = f1.await => x,

        // normal with cond
        x = f2.await if true => x,

        // multiple futures
        // question: should x be `Either`?
        // x = f3.await, f4.await => x,

        // multiple futures with cond
        // x = f3.await if true, f4.await if false => x,

        // multiple futures with ultra short
        // f3.await, f4.await => _,

        // shorthand
        // f5.await => 1,

        // shorthand with cond
        // f5.await if true => 1,

        // shorthand match
        // f6.await {
        //     Ok(x) => x,
        //     Err(e) => 0,
        // },
        // shorthand match with cond
        // f6.await if true {
        //     Ok(x) => x,
        //     Err(e) => 0,
        // },
    };
    println!("res: {:?}", res);
}
