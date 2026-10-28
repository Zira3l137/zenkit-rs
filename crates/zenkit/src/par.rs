//! Runs independent work items on rayon's thread pool when the `parallel` feature is on, and on the
//! calling thread otherwise. Results always come back in input order.

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Maps every item through `f` and stops at the first error.
pub(crate) fn try_map<T, U, E, F>(items: Vec<T>, f: F) -> Result<Vec<U>, E>
where
    T: Send,
    U: Send,
    E: Send,
    F: Fn(T) -> Result<U, E> + Sync + Send,
{
    #[cfg(feature = "parallel")]
    return items.into_par_iter().map(f).collect();

    #[cfg(not(feature = "parallel"))]
    items.into_iter().map(f).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_input_order() {
        let out: Result<Vec<_>, ()> = try_map((0..1000).collect(), |i| Ok(i * 2));
        assert_eq!(out.unwrap(), (0..1000).map(|i| i * 2).collect::<Vec<_>>());
    }

    #[test]
    fn propagates_errors() {
        let out = try_map((0..100).collect(), |i| if i == 42 { Err(i) } else { Ok(i) });
        assert_eq!(out, Err(42));
    }
}
