/*!
 * SOURCE OF TRUTH KEYWORDS: TsNumber, TsNumberOpt, bigint_policy, Millis
 * WHAT:  The TypeScript rendering used for every 64-bit integer we export.
 * WHY:   specta refuses to export i64/u64 by default, because JavaScript's
 *        `number` silently loses integer precision above 2^53 and a rounded id
 *        or ledger amount is a real hazard. It is not a hazard here, and the
 *        alternatives are both worse for a local app: `bigint` turns every date
 *        calculation in the frontend into a conversion, and `string` turns it
 *        into a parse.
 *
 *        Everything this app exports as a 64-bit integer is a millisecond
 *        timestamp (~1.8e12), a duration, a byte count or a word count. The
 *        largest of those is four orders of magnitude below the 2^53 limit, and
 *        ipc/bindings.rs asserts it. So `number` is both safe and the honest
 *        representation.
 *
 *        This alias exists so that decision is made ONCE and is greppable,
 *        rather than being re-argued at each of the thirty fields that need it.
 *        A new 64-bit field without it fails the bindings export loudly, which
 *        is the right place to notice.
 * WHERE: Applied as `#[specta(type = TsNumber)]` on 64-bit fields throughout
 *        types/, and `Option<TsNumber>` on nullable ones.
 */

/// Renders an i64/u64 field as TypeScript `number`. See the module WHY.
pub use specta_typescript::Number as TsNumber;
