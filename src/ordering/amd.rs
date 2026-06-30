//! Approximate Minimum Degree (AMD) ordering — stdlib-only.
//!
//! A faithful port of the canonical quotient-graph AMD (Amestoy, Davis & Duff
//! 1996), structured after Tim Davis's `cs_amd` (CSparse / "Direct Methods for
//! Sparse Linear Systems", §7.1). The algorithm and variable names are kept
//! close to that reference so the port is verifiable against it.
//!
//! It works entirely on `&Pattern` (the frozen contract input): the full
//! symmetric sparsity pattern in CSC, diagonal omitted — which is exactly the
//! matrix `C = A + Aᵀ` that `cs_amd` builds for a symmetric (Cholesky)
//! ordering, so we feed the pattern in directly with elbow room appended.
//!
//! Key ideas (why this is cheap enough for the 2 s cap):
//! - The eliminated graph is stored as a *quotient graph*: cliques produced by
//!   elimination are represented by "elements" rather than materialized, so the
//!   structure never blows up to O(fill).
//! - Degrees are *approximate* (an upper bound computed from element set
//!   differences), which keeps each pivot's update near-linear instead of
//!   re-scanning the true neighborhood.
//! - Mass elimination, supernode (indistinguishable-variable) detection, and
//!   aggressive element absorption collapse work further.
//!
//! Everything here is deterministic: no randomness, no hash-map iteration; the
//! "hash" is fixed integer arithmetic into fixed buckets. Two runs on the same
//! pattern produce byte-identical output, as the determinism gate requires.

use crate::Pattern;

/// CSparse's `CS_FLIP`: an involution mapping an index `i ≥ 0` to a negative
/// sentinel and back. Used to mark dead nodes/elements and assembly-tree
/// parents in place. `flip(flip(i)) == i`.
#[inline]
fn flip(i: isize) -> isize {
    -i - 2
}

/// Reset the `w` mark array when the running `mark` counter would overflow or
/// is too small to be a valid "cleared" sentinel. Mirrors `cs_wclear`. After
/// this returns `mark`, every live `w[k] < mark` holds.
fn wclear(mark: isize, lemax: isize, w: &mut [isize], n: usize) -> isize {
    if mark < 2 || (mark + lemax < 0) {
        for wk in w.iter_mut().take(n) {
            if *wk != 0 {
                *wk = 1;
            }
        }
        return 2;
    }
    mark
}

/// Iterative postorder depth-first traversal of the assembly tree, rooted at
/// `j`. Writes nodes into `post[k..]` in postorder and returns the next free
/// `k`. `stack` is scratch of length ≥ n+1. Mirrors `cs_tdfs`.
fn tdfs(
    j: isize,
    mut k: isize,
    head: &mut [isize],
    next: &[isize],
    post: &mut [isize],
    stack: &mut [isize],
) -> isize {
    let mut top: isize = 0;
    stack[0] = j;
    while top >= 0 {
        let p = stack[top as usize];
        let i = head[p as usize];
        if i == -1 {
            top -= 1; // p has no unordered children left
            post[k as usize] = p;
            k += 1;
        } else {
            head[p as usize] = next[i as usize]; // remove i from children of p
            top += 1;
            stack[top as usize] = i; // descend into child i
        }
    }
    k
}

/// Compute an AMD fill-reducing elimination order for `pattern`.
///
/// Returns `perm` where `perm[k]` is the original index eliminated k-th — a
/// bijection of `0..n`.
pub fn order(pattern: &Pattern) -> Vec<usize> {
    let n = pattern.n;
    if n == 0 {
        return Vec::new();
    }
    let n_i = n as isize;

    // --- Build C with elbow room ------------------------------------------
    // C is the input pattern itself (already A+Aᵀ, diagonal-free, sorted CSC).
    // `cs_amd` needs slack in the row-index array for newly formed elements;
    // `t = cnz + cnz/5 + 2n` is the same allowance CSparse appends.
    let cnz0 = pattern.row_idx.len();
    let nzmax = (cnz0 + cnz0 / 5 + 2 * n).max(1);
    let mut ci: Vec<isize> = Vec::with_capacity(nzmax);
    ci.extend(pattern.row_idx.iter().map(|&x| x as isize));
    ci.resize(nzmax, 0);
    // Cp gets one extra slot for the dummy element `n`; we mutate it in place.
    let mut cp: Vec<isize> = pattern.col_ptr.iter().map(|&x| x as isize).collect();

    // Dense-node threshold: variables with degree above this are not ordered by
    // min-degree but swept into the dummy element `n` (ordered late). Matches
    // CSparse: max(16, 10·√n), capped at n-2.
    let mut dense = ((10.0 * (n as f64).sqrt()).floor() as isize).max(16);
    if n_i - 2 < dense {
        dense = n_i - 2;
    }

    // --- Workspace (length n+1 each) --------------------------------------
    let np1 = n + 1;
    let mut len = vec![0isize; np1]; // len[i] = size of node/element i's list
    let mut nv = vec![0isize; np1]; // # original nodes a supervar/element holds
    let mut next = vec![0isize; np1]; // degree-list / hash-bucket successor
    let mut head = vec![0isize; np1]; // head[d] = first node of degree-d list
    let mut elen = vec![0isize; np1]; // elen[i] = |Ei| (#elements adjacent to i)
    let mut degree = vec![0isize; np1]; // (approximate) external degree
    let mut w = vec![0isize; np1]; // element liveness / set-difference marks
    let mut hhead = vec![0isize; np1]; // hash bucket heads (supernode detection)
    let mut last = vec![0isize; np1]; // degree-list predecessor / saved hash
    let mut p_out = vec![0isize; np1]; // postorder result

    let mut cnz = cnz0 as isize;

    for k in 0..n {
        len[k] = cp[k + 1] - cp[k];
    }
    len[n] = 0;

    for i in 0..=n {
        head[i] = -1;
        last[i] = -1;
        next[i] = -1;
        hhead[i] = -1;
        nv[i] = 1;
        w[i] = 1;
        elen[i] = 0;
        degree[i] = len[i];
    }
    let mut mark = wclear(0, 0, &mut w, n);
    elen[n] = -2; // n is a dead element (the dummy / dense sink)
    cp[n] = -1; // n is a root of the assembly tree
    w[n] = 0;

    let mut nel: isize = 0; // # nodes eliminated so far
    let mut mindeg: isize = 0; // current minimum degree
    let mut lemax: isize = 0; // max |Lk| seen (for wclear budgeting)

    // --- Initialize degree lists ------------------------------------------
    for i in 0..n {
        let d = degree[i];
        if d == 0 {
            // empty node: eliminate immediately, it makes no fill
            elen[i] = -2;
            nel += 1;
            cp[i] = -1;
            w[i] = 0;
        } else if d > dense {
            // dense node: absorb into the dummy element n (ordered last)
            nv[i] = 0;
            elen[i] = -1;
            nel += 1;
            cp[i] = flip(n_i);
            nv[n] += 1;
        } else {
            let hd = head[d as usize];
            if hd != -1 {
                last[hd as usize] = i as isize;
            }
            next[i] = head[d as usize];
            head[d as usize] = i as isize;
        }
    }

    // --- Main elimination loop --------------------------------------------
    while nel < n_i {
        // Select the node k of minimum approximate degree.
        let mut k: isize = -1;
        while mindeg < n_i {
            k = head[mindeg as usize];
            if k != -1 {
                break;
            }
            mindeg += 1;
        }
        if next[k as usize] != -1 {
            last[next[k as usize] as usize] = -1;
        }
        head[mindeg as usize] = next[k as usize]; // remove k from its degree list
        let elenk = elen[k as usize]; // |Ek|
        let mut nvk = nv[k as usize]; // # nodes k represents
        nel += nvk;

        // --- Garbage collection: compact Ci when it runs out of room ------
        if elenk > 0 && cnz + mindeg >= nzmax as isize {
            for j in 0..n {
                let p = cp[j];
                if p >= 0 {
                    // j is live: stash its first entry, flag its head with flip(j)
                    cp[j] = ci[p as usize];
                    ci[p as usize] = flip(j as isize);
                }
            }
            let mut q: isize = 0;
            let mut p: isize = 0;
            while p < cnz {
                let j = flip(ci[p as usize]);
                p += 1;
                if j >= 0 {
                    // found object j: restore and slide it down to q
                    ci[q as usize] = cp[j as usize];
                    cp[j as usize] = q;
                    q += 1;
                    let copies = len[j as usize] - 1;
                    let mut c = 0;
                    while c < copies {
                        ci[q as usize] = ci[p as usize];
                        q += 1;
                        p += 1;
                        c += 1;
                    }
                }
            }
            cnz = q;
        }

        // --- Construct the new element Lk ---------------------------------
        let mut dk: isize = 0;
        nv[k as usize] = -nvk; // flag k as being assembled into Lk
        let mut p = cp[k as usize];
        let pk1 = if elenk == 0 { p } else { cnz }; // in place if k has no elements
        let mut pk2 = pk1;
        for k1 in 1..=(elenk + 1) {
            let e;
            let mut pj;
            let ln;
            if k1 > elenk {
                e = k; // assemble the plain nodes listed in k
                pj = p;
                ln = len[k as usize] - elenk;
            } else {
                e = ci[p as usize]; // assemble nodes of element e adjacent to k
                p += 1;
                pj = cp[e as usize];
                ln = len[e as usize];
            }
            for _ in 1..=ln {
                let i = ci[pj as usize];
                pj += 1;
                let nvi = nv[i as usize];
                if nvi <= 0 {
                    continue; // i is dead or already in Lk
                }
                dk += nvi; // |Lk| grows by size of i
                nv[i as usize] = -nvi; // flag i as in Lk
                ci[pk2 as usize] = i;
                pk2 += 1;
                // remove i from its degree list
                if next[i as usize] != -1 {
                    last[next[i as usize] as usize] = last[i as usize];
                }
                if last[i as usize] != -1 {
                    next[last[i as usize] as usize] = next[i as usize];
                } else {
                    head[degree[i as usize] as usize] = next[i as usize];
                }
            }
            if e != k {
                cp[e as usize] = flip(k); // absorb element e into k
                w[e as usize] = 0;
            }
        }
        if elenk != 0 {
            cnz = pk2; // Ci[cnz..] is free again
        }
        degree[k as usize] = dk; // |Lk|
        cp[k as usize] = pk1;
        len[k as usize] = pk2 - pk1;
        elen[k as usize] = -2; // k is now an element

        // --- Scan 1: compute |Le \ Lk| for each element e touching Lk ------
        mark = wclear(mark, lemax, &mut w, n);
        for pk in pk1..pk2 {
            let i = ci[pk as usize];
            let eln = elen[i as usize];
            if eln <= 0 {
                continue;
            }
            let nvi = -nv[i as usize];
            let wnvi = mark - nvi;
            let mut pp = cp[i as usize];
            let end = cp[i as usize] + eln - 1;
            while pp <= end {
                let e = ci[pp as usize];
                if w[e as usize] >= mark {
                    w[e as usize] -= nvi; // decrement |Le \ Lk|
                } else if w[e as usize] != 0 {
                    w[e as usize] = degree[e as usize] + wnvi; // first sighting
                }
                pp += 1;
            }
        }

        // --- Scan 2: approximate degree update for each i in Lk -----------
        for pk in pk1..pk2 {
            let i = ci[pk as usize];
            let p1 = cp[i as usize];
            let p2 = p1 + elen[i as usize] - 1;
            let mut pn = p1;
            let mut h: isize = 0; // hash of i's adjacency (for supernode detection)
            let mut d: isize = 0; // new approximate degree of i
            let mut pp = p1;
            while pp <= p2 {
                let e = ci[pp as usize];
                if w[e as usize] != 0 {
                    let dext = w[e as usize] - mark; // |Le \ Lk|
                    if dext > 0 {
                        d += dext;
                        ci[pn as usize] = e; // keep e in Ei
                        pn += 1;
                        h += e;
                    } else {
                        cp[e as usize] = flip(k); // aggressive absorption e → k
                        w[e as usize] = 0;
                    }
                }
                pp += 1;
            }
            elen[i as usize] = pn - p1 + 1; // |Ei|
            let p3 = pn;
            let p4 = p1 + len[i as usize];
            let mut pp2 = p2 + 1;
            while pp2 < p4 {
                let j = ci[pp2 as usize];
                let nvj = nv[j as usize];
                if nvj > 0 {
                    d += nvj; // count surviving plain neighbor j
                    ci[pn as usize] = j;
                    pn += 1;
                    h += j;
                }
                pp2 += 1;
            }
            if d == 0 {
                // mass elimination: i has the same neighborhood as k, fold it in
                cp[i as usize] = flip(k);
                let nvi = -nv[i as usize];
                dk -= nvi;
                nvk += nvi;
                nel += nvi;
                nv[i as usize] = 0;
                elen[i as usize] = -1;
            } else {
                if degree[i as usize] > d {
                    degree[i as usize] = d; // tighten the approximate degree
                }
                // rotate k to the front of Ei, then bucket i by its hash
                ci[pn as usize] = ci[p3 as usize];
                ci[p3 as usize] = ci[p1 as usize];
                ci[p1 as usize] = k;
                len[i as usize] = pn - p1 + 1;
                let hh = h.rem_euclid(n_i) as usize;
                next[i as usize] = hhead[hh];
                hhead[hh] = i;
                last[i as usize] = hh as isize;
            }
        }
        degree[k as usize] = dk;
        if dk > lemax {
            lemax = dk;
        }
        mark = wclear(mark + lemax, lemax, &mut w, n);

        // --- Supernode detection: merge indistinguishable variables -------
        for pk in pk1..pk2 {
            let mut i = ci[pk as usize];
            if nv[i as usize] >= 0 {
                continue; // i already dead/merged
            }
            let h = last[i as usize];
            i = hhead[h as usize]; // walk the whole hash bucket
            hhead[h as usize] = -1;
            while i != -1 && next[i as usize] != -1 {
                let ln = len[i as usize];
                let eln = elen[i as usize];
                let mut pp = cp[i as usize] + 1;
                let endp = cp[i as usize] + ln - 1;
                while pp <= endp {
                    w[ci[pp as usize] as usize] = mark; // mark i's adjacency
                    pp += 1;
                }
                let mut jlast = i;
                let mut j = next[i as usize];
                while j != -1 {
                    let mut ok = (len[j as usize] == ln) && (elen[j as usize] == eln);
                    let mut pp2 = cp[j as usize] + 1;
                    let endp2 = cp[j as usize] + ln - 1;
                    while ok && pp2 <= endp2 {
                        if w[ci[pp2 as usize] as usize] != mark {
                            ok = false; // j differs from i
                        }
                        pp2 += 1;
                    }
                    if ok {
                        // j is indistinguishable from i: absorb it
                        cp[j as usize] = flip(i);
                        nv[i as usize] += nv[j as usize];
                        nv[j as usize] = 0;
                        elen[j as usize] = -1;
                        j = next[j as usize]; // unlink j from the bucket
                        next[jlast as usize] = j;
                    } else {
                        jlast = j;
                        j = next[j as usize];
                    }
                }
                i = next[i as usize];
                mark += 1;
            }
        }

        // --- Finalize Lk: restore survivors and re-insert in degree lists -
        let mut p = pk1;
        for pk in pk1..pk2 {
            let i = ci[pk as usize];
            let nvi = -nv[i as usize];
            if nvi <= 0 {
                continue; // i was absorbed
            }
            nv[i as usize] = nvi; // restore nv[i]
            // external degree of i, bounded by the # of remaining variables
            let mut d = degree[i as usize] + dk - nvi;
            if d > n_i - nel - nvi {
                d = n_i - nel - nvi;
            }
            let hd = head[d as usize];
            if hd != -1 {
                last[hd as usize] = i;
            }
            next[i as usize] = head[d as usize];
            last[i as usize] = -1;
            head[d as usize] = i;
            if d < mindeg {
                mindeg = d; // a smaller degree may now be available
            }
            degree[i as usize] = d;
            ci[p as usize] = i; // compact Lk
            p += 1;
        }
        nv[k as usize] = nvk; // # nodes absorbed into element k
        len[k as usize] = p - pk1;
        if len[k as usize] == 0 {
            cp[k as usize] = -1; // k is a root of the assembly tree
            w[k as usize] = 0;
        }
        if elenk != 0 {
            cnz = p;
        }
    }

    // --- Postorder the assembly tree to get the elimination order ---------
    for cpi in cp.iter_mut().take(n) {
        *cpi = flip(*cpi); // undo the flip-marking on parents
    }
    for h in head.iter_mut().take(np1) {
        *h = -1;
    }
    // Place each variable in its parent's child list.
    for j in (0..=n).rev() {
        if nv[j] > 0 {
            continue; // skip elements
        }
        next[j] = head[cp[j] as usize];
        head[cp[j] as usize] = j as isize;
    }
    // Place each element under its parent (roots have cp == -1).
    for e in (0..=n).rev() {
        if nv[e] <= 0 {
            continue; // skip variables
        }
        if cp[e] != -1 {
            next[e] = head[cp[e] as usize];
            head[cp[e] as usize] = e as isize;
        }
    }
    let mut k_post: isize = 0;
    for i in 0..=n {
        if cp[i] == -1 {
            k_post = tdfs(i as isize, k_post, &mut head, &next, &mut p_out, &mut w);
        }
    }

    // p_out is a postorder of 0..=n; the dummy element `n` is included. Keep the
    // real variables (index < n) in postorder — that is the elimination order.
    let mut perm: Vec<usize> = Vec::with_capacity(n);
    for &v in p_out.iter() {
        if v >= 0 && (v as usize) < n {
            perm.push(v as usize);
        }
    }
    debug_assert_eq!(perm.len(), n, "AMD postorder must cover every variable");
    perm
}
