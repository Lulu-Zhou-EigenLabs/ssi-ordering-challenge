use crate::pattern::Pattern;

pub fn order(pattern: &Pattern) -> Vec<usize> {
    let n = pattern.n;
    if n == 0 {
        return vec![];
    }

    let adj = build_adj(pattern);

    if let Some(hub) = detect_arrow(&adj, n) {
        return order_arrow(&adj, n, hub);
    }

    if is_grid_like(&adj, n) {
        return order_nd(&adj, n);
    }

    order_amd(&adj, &(0..n).collect::<Vec<_>>())
}

fn build_adj(pattern: &Pattern) -> Vec<Vec<usize>> {
    let n = pattern.n;
    let mut adj = Vec::with_capacity(n);
    for j in 0..n {
        adj.push(pattern.col(j).to_vec());
    }
    adj
}

fn detect_arrow(adj: &[Vec<usize>], n: usize) -> Option<usize> {
    if n < 100 {
        return None;
    }
    let mut max_deg = 0;
    let mut max_v = 0;
    let mut second_deg = 0;
    for v in 0..n {
        let d = adj[v].len();
        if d > max_deg {
            second_deg = max_deg;
            max_deg = d;
            max_v = v;
        } else if d > second_deg {
            second_deg = d;
        }
    }
    if max_deg > n / 2 && max_deg > 10 * (second_deg + 1) {
        Some(max_v)
    } else {
        None
    }
}

fn is_grid_like(adj: &[Vec<usize>], n: usize) -> bool {
    if n < 1000 {
        return false;
    }
    let max_deg = adj.iter().map(|a| a.len()).max().unwrap_or(0);
    if max_deg > 6 {
        return false;
    }
    let avg_deg: f64 = adj.iter().map(|a| a.len()).sum::<usize>() as f64 / n as f64;
    avg_deg >= 3.0
}

fn order_arrow(adj: &[Vec<usize>], n: usize, hub: usize) -> Vec<usize> {
    let sub_nodes: Vec<usize> = (0..n).filter(|&v| v != hub).collect();
    let mut perm = order_amd(adj, &sub_nodes);
    perm.push(hub);
    perm
}

fn order_nd(adj: &[Vec<usize>], n: usize) -> Vec<usize> {
    let alive: Vec<usize> = (0..n).collect();
    let mut perm = Vec::with_capacity(n);
    nd_recurse(adj, &alive, &mut perm);
    perm
}

const ND_THRESHOLD: usize = 200;

fn nd_recurse(adj: &[Vec<usize>], alive: &[usize], perm: &mut Vec<usize>) {
    let ns = alive.len();
    if ns <= ND_THRESHOLD {
        let local_perm = order_amd(adj, alive);
        perm.extend_from_slice(&local_perm);
        return;
    }

    let (part_a, part_b, separator) = nd_bisect(adj, alive);

    if separator.is_empty() || part_a.is_empty() || part_b.is_empty() {
        let local_perm = order_amd(adj, alive);
        perm.extend_from_slice(&local_perm);
        return;
    }

    let balance = part_a.len().min(part_b.len()) as f64 / (part_a.len() + part_b.len()) as f64;
    if balance < 0.1 || separator.len() > ns / 2 {
        let local_perm = order_amd(adj, alive);
        perm.extend_from_slice(&local_perm);
        return;
    }

    nd_recurse(adj, &part_a, perm);
    nd_recurse(adj, &part_b, perm);

    let sep_perm = order_amd(adj, &separator);
    perm.extend_from_slice(&sep_perm);
}

fn nd_bisect(adj: &[Vec<usize>], alive: &[usize]) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let ns = alive.len();
    let full_n = adj.len();

    let mut local_id = vec![usize::MAX; full_n];
    for (i, &v) in alive.iter().enumerate() {
        local_id[v] = i;
    }

    let mut local_adj: Vec<Vec<usize>> = Vec::with_capacity(ns);
    for &v in alive {
        let mut nbrs = Vec::new();
        for &u in &adj[v] {
            if local_id[u] != usize::MAX {
                nbrs.push(local_id[u]);
            }
        }
        local_adj.push(nbrs);
    }

    let start = pseudo_peripheral_local(&local_adj, ns);
    let end = pseudo_peripheral_from(&local_adj, ns, start);

    let levels_s = bfs_levels_local(&local_adj, ns, start);
    let levels_e = bfs_levels_local(&local_adj, ns, end);

    let mut best_partition: Option<(Vec<usize>, Vec<usize>, Vec<usize>)> = None;
    let mut best_sep_score = f64::MAX;

    // Strategy 1 & 2: level-set separator from start and end BFS
    for levels in [&levels_s, &levels_e] {
        let (mid, _) = best_level_cut(levels, ns);
        if mid == 0 {
            continue;
        }
        let mut pa = Vec::new();
        let mut pb = Vec::new();
        let mut sep = Vec::new();
        for (i, &v) in alive.iter().enumerate() {
            if levels[i] < mid {
                pa.push(v);
            } else if levels[i] > mid {
                pb.push(v);
            } else {
                sep.push(v);
            }
        }
        let score = partition_score(&pa, &pb, &sep);
        if score < best_sep_score && !pa.is_empty() && !pb.is_empty() && !sep.is_empty() {
            best_sep_score = score;
            best_partition = Some((pa, pb, sep));
        }
    }

    // Strategy 3: distance-difference bisection
    {
        let mut diff: Vec<(i64, usize)> = (0..ns)
            .map(|i| (levels_s[i] as i64 - levels_e[i] as i64, i))
            .collect();
        diff.sort_unstable();

        let half = ns / 2;
        let mut side = vec![0u8; ns];
        for i in 0..ns {
            side[diff[i].1] = if i < half { 0 } else { 1 };
        }
        for i in 0..ns {
            if side[i] == 2 {
                continue;
            }
            let my_side = side[i];
            for &u in &local_adj[i] {
                if side[u] != 2 && side[u] != my_side {
                    side[i] = 2;
                    break;
                }
            }
        }

        let mut pa = Vec::new();
        let mut pb = Vec::new();
        let mut sep = Vec::new();
        for (i, &v) in alive.iter().enumerate() {
            match side[i] {
                0 => pa.push(v),
                1 => pb.push(v),
                _ => sep.push(v),
            }
        }
        let score = partition_score(&pa, &pb, &sep);
        if score < best_sep_score && !pa.is_empty() && !pb.is_empty() && !sep.is_empty() {
            best_sep_score = score;
            best_partition = Some((pa, pb, sep));
        }
    }

    // Strategy 4: BFS from midpoint of diameter
    let max_level_s = levels_s.iter().copied().max().unwrap_or(0);
    {
        let target_level = max_level_s / 2;
        if target_level > 0 && target_level < max_level_s {
            let mut mid_vertex = None;
            let mut mid_deg = usize::MAX;
            for i in 0..ns {
                if levels_s[i] == target_level && local_adj[i].len() < mid_deg {
                    mid_deg = local_adj[i].len();
                    mid_vertex = Some(i);
                }
            }
            if let Some(mv) = mid_vertex {
                let levels_m = bfs_levels_local(&local_adj, ns, mv);
                let (mid, _) = best_level_cut(&levels_m, ns);
                if mid > 0 {
                    let mut pa = Vec::new();
                    let mut pb = Vec::new();
                    let mut sep = Vec::new();
                    for (i, &v) in alive.iter().enumerate() {
                        if levels_m[i] < mid {
                            pa.push(v);
                        } else if levels_m[i] > mid {
                            pb.push(v);
                        } else {
                            sep.push(v);
                        }
                    }
                    let score = partition_score(&pa, &pb, &sep);
                    if score < best_sep_score && !pa.is_empty() && !pb.is_empty() && !sep.is_empty() {
                        best_sep_score = score;
                        best_partition = Some((pa, pb, sep));
                    }
                }
            }
        }
    }

    let (mut part_a, mut part_b, mut separator) = match best_partition {
        Some(p) => p,
        None => return (vec![], vec![], alive.to_vec()),
    };

    refine_separator(&local_adj, ns, &mut part_a, &mut part_b, &mut separator, alive, &local_id);

    (part_a, part_b, separator)
}

fn partition_score(part_a: &[usize], part_b: &[usize], sep: &[usize]) -> f64 {
    let ns = part_a.len() + part_b.len() + sep.len();
    if ns == 0 || part_a.is_empty() || part_b.is_empty() {
        return f64::MAX;
    }
    let balance = part_a.len().min(part_b.len()) as f64 / (part_a.len() + part_b.len()) as f64;
    if balance < 0.10 {
        return f64::MAX;
    }
    sep.len() as f64 / ns as f64 + 0.1 * (0.5 - balance).abs()
}

fn best_level_cut(levels: &[usize], ns: usize) -> (usize, f64) {
    let max_level = levels.iter().copied().max().unwrap_or(0);
    if max_level < 3 {
        return (0, f64::MAX);
    }

    let mut best_score = f64::MAX;
    let mut best_mid = max_level / 2;

    for mid in 1..max_level {
        let sep_size = levels.iter().filter(|&&l| l == mid).count();
        let a_size = levels.iter().filter(|&&l| l < mid).count();
        let b_size = levels.iter().filter(|&&l| l > mid).count();
        if a_size == 0 || b_size == 0 {
            continue;
        }
        let balance = a_size.min(b_size) as f64 / (a_size + b_size) as f64;
        if balance < 0.10 {
            continue;
        }
        let score = sep_size as f64 / ns as f64 + 0.15 * (0.5 - balance).abs();
        if score < best_score {
            best_score = score;
            best_mid = mid;
        }
    }

    (best_mid, best_score)
}

fn refine_separator(
    local_adj: &[Vec<usize>],
    ns: usize,
    part_a: &mut Vec<usize>,
    part_b: &mut Vec<usize>,
    separator: &mut Vec<usize>,
    alive: &[usize],
    local_id: &[usize],
) {
    let mut side = vec![0u8; ns];
    for &v in part_a.iter() {
        side[local_id[v]] = 0;
    }
    for &v in part_b.iter() {
        side[local_id[v]] = 1;
    }
    for &v in separator.iter() {
        side[local_id[v]] = 2;
    }

    for _pass in 0..20 {
        let mut improved = false;

        for i in 0..ns {
            if side[i] != 2 {
                continue;
            }
            let mut a_nbrs = 0;
            let mut b_nbrs = 0;
            for &u in &local_adj[i] {
                match side[u] {
                    0 => a_nbrs += 1,
                    1 => b_nbrs += 1,
                    _ => {}
                }
            }
            if a_nbrs > 0 && b_nbrs == 0 {
                side[i] = 0;
                improved = true;
            } else if b_nbrs > 0 && a_nbrs == 0 {
                side[i] = 1;
                improved = true;
            }
        }

        if !improved {
            break;
        }

        for i in 0..ns {
            if side[i] == 2 {
                continue;
            }
            let my_side = side[i];
            let other_side = 1 - my_side;
            for &u in &local_adj[i] {
                if side[u] == other_side {
                    side[i] = 2;
                    break;
                }
            }
        }
    }

    part_a.clear();
    part_b.clear();
    separator.clear();
    for &v in alive {
        match side[local_id[v]] {
            0 => part_a.push(v),
            1 => part_b.push(v),
            _ => separator.push(v),
        }
    }
}

fn pseudo_peripheral_local(adj: &[Vec<usize>], n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut start = 0;
    let mut min_deg = adj[0].len();
    for i in 1..n {
        if adj[i].len() < min_deg {
            min_deg = adj[i].len();
            start = i;
        }
    }
    pseudo_peripheral_from(adj, n, start)
}

fn pseudo_peripheral_from(adj: &[Vec<usize>], n: usize, mut start: usize) -> usize {
    for _ in 0..5 {
        let levels = bfs_levels_local(adj, n, start);
        let max_level = levels.iter().copied().max().unwrap_or(0);
        let mut farthest = start;
        let mut min_deg_at_max = usize::MAX;
        for i in 0..n {
            if levels[i] == max_level && adj[i].len() < min_deg_at_max {
                min_deg_at_max = adj[i].len();
                farthest = i;
            }
        }
        if farthest == start {
            break;
        }
        start = farthest;
    }
    start
}

fn bfs_levels_local(adj: &[Vec<usize>], n: usize, start: usize) -> Vec<usize> {
    let mut levels = vec![usize::MAX; n];
    levels[start] = 0;
    let mut queue = Vec::with_capacity(n);
    queue.push(start);
    let mut head = 0;
    while head < queue.len() {
        let v = queue[head];
        head += 1;
        let next_level = levels[v] + 1;
        for &u in &adj[v] {
            if levels[u] == usize::MAX {
                levels[u] = next_level;
                queue.push(u);
            }
        }
    }
    for l in levels.iter_mut() {
        if *l == usize::MAX {
            *l = 0;
        }
    }
    levels
}

fn order_amd(full_adj: &[Vec<usize>], alive: &[usize]) -> Vec<usize> {
    let ns = alive.len();
    if ns == 0 {
        return vec![];
    }
    if ns == 1 {
        return vec![alive[0]];
    }

    let full_n = full_adj.len();
    let mut local_id = vec![usize::MAX; full_n];
    for (i, &v) in alive.iter().enumerate() {
        local_id[v] = i;
    }

    let mut adj: Vec<Vec<usize>> = Vec::with_capacity(ns);
    for &v in alive {
        let mut nbrs = Vec::new();
        for &u in &full_adj[v] {
            if local_id[u] != usize::MAX {
                nbrs.push(local_id[u]);
            }
        }
        nbrs.sort_unstable();
        adj.push(nbrs);
    }

    let mut degree: Vec<usize> = (0..ns).map(|i| adj[i].len()).collect();
    let mut eliminated = vec![false; ns];
    let mut perm = Vec::with_capacity(ns);

    let mut bucket: Vec<Vec<usize>> = vec![Vec::new(); ns + 1];
    for i in 0..ns {
        bucket[degree[i]].push(i);
    }
    let mut min_deg = 0;

    while perm.len() < ns {
        while min_deg <= ns && bucket[min_deg].is_empty() {
            min_deg += 1;
        }
        if min_deg > ns {
            break;
        }

        let fill_threshold = if ns <= 200 { 20 } else if ns <= 500 { 15 } else { 10 };
        let v = if min_deg <= fill_threshold && bucket[min_deg].len() > 1 {
            pick_min_fill(&adj, &bucket[min_deg], &eliminated)
        } else {
            bucket[min_deg].iter().copied().min().unwrap()
        };

        bucket[min_deg].retain(|&x| x != v);
        eliminated[v] = true;
        perm.push(alive[v]);

        let neigh: Vec<usize> = adj[v].iter().copied().filter(|&u| !eliminated[u]).collect();

        for &u in &neigh {
            adj[u].retain(|&x| x != v);
        }

        for i in 0..neigh.len() {
            for j in (i + 1)..neigh.len() {
                let (a, b) = (neigh[i], neigh[j]);
                if !adj[a].contains(&b) {
                    adj[a].push(b);
                    adj[b].push(a);
                }
            }
        }

        for &u in &neigh {
            let old_deg = degree[u];
            let new_deg = adj[u].len();
            if old_deg != new_deg {
                bucket[old_deg].retain(|&x| x != u);
                degree[u] = new_deg;
                if new_deg <= ns {
                    bucket[new_deg].push(u);
                }
                if new_deg < min_deg {
                    min_deg = new_deg;
                }
            }
        }

        adj[v].clear();
    }
    perm
}

fn pick_min_fill(adj: &[Vec<usize>], candidates: &[usize], eliminated: &[bool]) -> usize {
    let mut best = candidates[0];
    let mut best_fill = usize::MAX;

    for &v in candidates {
        let neigh: Vec<usize> = adj[v].iter().copied().filter(|&u| !eliminated[u]).collect();
        let mut fill = 0;
        let mut exceeded = false;
        for i in 0..neigh.len() {
            for j in (i + 1)..neigh.len() {
                if !adj[neigh[i]].contains(&neigh[j]) {
                    fill += 1;
                    if fill > best_fill {
                        exceeded = true;
                        break;
                    }
                }
            }
            if exceeded {
                break;
            }
        }
        if !exceeded && (fill < best_fill || (fill == best_fill && v < best)) {
            best_fill = fill;
            best = v;
        }
    }
    best
}
