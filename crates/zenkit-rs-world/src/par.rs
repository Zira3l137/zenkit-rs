//! Runs two independent jobs on rayon's thread pool when the `parallel` feature is on, and one
//! after the other otherwise.

/// Runs `a` and `b`, possibly at the same time, and gives both results.
///
/// Whatever either job logs goes to the caller's subscriber, in the caller's span, which usually
/// names the file being loaded, even when the job runs on another thread.
pub(crate) fn join<A, B, RA, RB>(a: A, b: B) -> (RA, RB)
where
    A: FnOnce() -> RA + Send,
    B: FnOnce() -> RB + Send,
    RA: Send,
    RB: Send,
{
    #[cfg(feature = "parallel")]
    {
        // A subscriber set with `with_default` only applies to the thread that set it.
        let dispatch = tracing::dispatcher::get_default(tracing::Dispatch::clone);
        let span = tracing::Span::current();
        let (dispatch_b, span_b) = (dispatch.clone(), span.clone());
        rayon::join(
            move || tracing::dispatcher::with_default(&dispatch, || span.in_scope(a)),
            move || tracing::dispatcher::with_default(&dispatch_b, || span_b.in_scope(b)),
        )
    }

    #[cfg(not(feature = "parallel"))]
    (a(), b())
}
