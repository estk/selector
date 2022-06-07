# Selector

An alternative select macro using tokio types, compare the syntax below. I encourage anyone interested to the following syntax freely with or without attribution.

Note that this is a proof of concept and should not be used as is.

<table>
<tr>
<td>

 `tokio::select`
 
</td><td>

`selector::select`
    
</td>
</tr>
<tr>
<td> 

```rust 
tokio::select! {
    _ = &mut delay, if !delay.is_elapsed() => {
        println!("operation timed out");
        1
    }
    _ = some_async_work() => {
        println!("operation completed");
        0
    }
}
``` 
</td>
<td>

```rust 
selector::select! {
    // short
    &mut delay.await if !delay.is_elapsed() => {
        println!("operation timed out");
        1
    },

    // dry
    async {2}.await => _,

    // flexible
    x = async { 6 }.await, async { 7 }.await => x,

    // powerful
    f10.await {
        Ok(x) => x,
        Err(_) => 0,
    },
};
``` 
 </td>
 </tr>
 </table>


## Notes

Syntax and machinery mostly copied from tokio, presented here as an example.

## License

MIT
