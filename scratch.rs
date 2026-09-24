use stellar_xdr::{ScVal, Limits, ReadXdr, WriteXdr};

fn main() {
    let non_canonical_hex = "000000010000000141010203"; // ScVal::Sym("A" + padding)
    let bytes = hex::decode(non_canonical_hex).unwrap();
    let b64 = base64::encode(&bytes);
    println!("Base64: {}", b64);
    
    match ScVal::from_xdr_base64(&b64, Limits::none()) {
        Ok(v) => {
            println!("Decoded: {:?}", v);
            let re_encoded = v.to_xdr_base64(Limits::none()).unwrap();
            println!("Re-encoded: {}", re_encoded);
            assert_ne!(b64, re_encoded);
            println!("Success!");
        },
        Err(e) => println!("Error: {:?}", e),
    }
}
