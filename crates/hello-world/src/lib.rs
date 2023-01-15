pub fn hello() -> String {
    "Hello, world!".to_string()
}

#[cfg(test)]
mod tests {
    use crate::hello;

    #[test]
    fn hello_begins_with_hello() {
        assert!(hello().starts_with("Hello"))
    }
}
