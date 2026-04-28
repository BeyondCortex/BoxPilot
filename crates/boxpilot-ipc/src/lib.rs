pub mod method;
pub use method::HelperMethod;

pub mod error;
pub use error::{HelperError, HelperResult};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
