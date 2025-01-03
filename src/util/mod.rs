mod call;
mod echo;
mod thunk;

pub use call::{Caller, OnceCaller, RefCaller, RetryCaller};
pub use echo::Echo;
pub use thunk::{FunkWorker, PunkWorker, Thunk, ThunkWorker};

#[cfg(feature = "retry")]
pub use retry::try_map_retryable;

use crate::hive::{Builder, Outcome, OutcomeBatch};

use std::fmt::Debug;

/// Convenience function that creates a `Hive` with `num_threads` worker threads that execute the
/// provided callable on the provided inputs and returns a `Vec` of the results.
///
/// The provided function should not panic. For falible operations, use one of the `try_map_*`
/// functions.
///
/// # Examples
///
/// ```
/// # fn main() {
/// let outputs: Vec<usize> = drudge::util::map(4, 2..9usize, |i| i + 1);
/// assert_eq!(outputs.into_iter().sum::<usize>(), 42);
/// # }
/// ```
pub fn map<I, O, Inputs, F>(num_threads: usize, inputs: Inputs, f: F) -> Vec<O>
where
    I: Send + Sync + 'static,
    O: Send + Sync + 'static,
    Inputs: IntoIterator<Item = I>,
    F: FnMut(I) -> O + Send + Sync + Clone + 'static,
{
    Builder::default()
        .num_threads(num_threads)
        .build_with(Caller::of(f))
        .map(inputs)
        .map(Outcome::unwrap)
        .collect()
}

/// Convenience function that creates a `Hive` with `num_threads` worker threads that execute the
/// provided callable on the provided inputs and returns a `Vec` of the results, or an error if any
/// of the tasks failed.
///
/// # Examples
///
/// ```
/// # use drudge::hive::OutcomeDerefStore;
/// # fn main() {
/// let result = drudge::util::try_map(4, 0..10, |i| {
///     if i == 5 {
///         Err("No fives allowed!")
///     } else {
///         Ok(i * i)
///     }
/// });
/// assert!(result.has_failures());
/// # }
/// ```
pub fn try_map<I, O, E, Inputs, F>(
    num_threads: usize,
    inputs: Inputs,
    f: F,
) -> OutcomeBatch<OnceCaller<I, O, E, F>>
where
    I: Send + Sync + 'static,
    O: Send + Sync + 'static,
    E: Send + Sync + Debug + 'static,
    Inputs: IntoIterator<Item = I>,
    F: FnMut(I) -> Result<O, E> + Send + Sync + Clone + 'static,
{
    Builder::default()
        .num_threads(num_threads)
        .build_with(OnceCaller::of(f))
        .map(inputs)
        .into()
}

#[cfg(test)]
mod tests {
    use crate::hive::{Outcome, OutcomeDerefStore, OutcomeStore};

    #[test]
    fn test_map() {
        let outputs = super::map(4, 0..100, |i| i + 1);
        assert_eq!(outputs, (1..=100).collect::<Vec<_>>());
    }

    #[test]
    fn test_try_map() {
        let result = super::try_map(
            4,
            0..100,
            |i| {
                if i == 50 {
                    Err("Fiddy!")
                } else {
                    Ok(i + 1)
                }
            },
        );
        assert!(result.has_failures());
        assert_eq!(1, result.num_failures());
        assert!(matches!(
            result.iter_failures().next().unwrap(),
            Outcome::Failure { .. }
        ));
        assert_eq!(99, result.num_successes());
        assert!(matches!(result.ok_or_unwrap_errors(true), Err(_)));
    }
}

#[cfg(feature = "retry")]
mod retry {
    use super::RetryCaller;
    use crate::hive::{Builder, OutcomeBatch};
    use crate::task::{ApplyError, Context};
    use std::fmt::Debug;

    /// Convenience function that creates a `Hive` with `num_threads` worker threads that execute the
    /// provided callable on the provided inputs and returns a `Vec` of the results, or an error if any
    /// of the tasks failed.
    ///
    /// A task that fails with an `ApplyError::Retryable` error will be retried up to `max_retries`
    /// times.
    ///
    /// # Examples
    ///
    /// ```
    /// # use drudge::hive::OutcomeDerefStore;
    /// # use drudge::task::ApplyError;
    /// # use drudge::util::RetryCaller;
    ///
    /// # fn main() {
    /// let result = drudge::util::try_map_retryable::<usize, usize, String, _, _>(4, 3, 0..10, |i, _| if i == 5 {
    ///     Err(ApplyError::NotRetryable { input: Some(i), error: "No fives allowed!".into() })
    /// } else if i == 7 {
    ///     Err(ApplyError::Retryable { input: i, error: "Re-roll a 7".into() })
    /// } else {
    ///     Ok(i * i)
    /// });
    /// assert!(result.has_failures());
    /// # }
    /// ```
    pub fn try_map_retryable<I, O, E, Inputs, F>(
        num_threads: usize,
        max_retries: u32,
        inputs: Inputs,
        f: F,
    ) -> OutcomeBatch<RetryCaller<I, O, E, F>>
    where
        I: Send + Sync + 'static,
        O: Send + Sync + 'static,
        E: Send + Sync + Debug + 'static,
        Inputs: IntoIterator<Item = I>,
        F: FnMut(I, &Context) -> Result<O, ApplyError<I, E>> + Send + Sync + Clone + 'static,
    {
        Builder::default()
            .num_threads(num_threads)
            .max_retries(max_retries)
            .build_with(RetryCaller::of(f))
            .map(inputs)
            .into()
    }

    #[cfg(test)]
    mod tests {
        use crate::hive::{Outcome, OutcomeDerefStore, OutcomeStore};
        use crate::task::ApplyError;

        #[test]
        fn test_try_map_retryable() {
            let result = super::try_map_retryable(4, 3, 0..100, |i, ctx| {
                if i != 50 {
                    Ok(i + 1)
                } else if ctx.attempt() == 3 {
                    Ok(500)
                } else {
                    Err(ApplyError::Retryable {
                        input: 50,
                        error: format!("Fiddy {}", ctx.attempt()),
                    })
                }
            });
            assert!(!result.has_failures());
        }

        #[test]
        fn test_try_map_retyrable_fail() {
            let result = super::try_map_retryable(4, 3, 0..100, |i, ctx| {
                if i != 50 {
                    Ok(i + 1)
                } else {
                    Err(ApplyError::Retryable {
                        input: 50,
                        error: format!("Fiddy {}", ctx.attempt()),
                    })
                }
            });
            assert!(result.has_failures());
            assert!(result.num_failures() == 1);
            assert!(matches!(
                result.iter_failures().next().unwrap(),
                Outcome::MaxRetriesAttempted { .. }
            ))
        }
    }
}
