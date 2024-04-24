use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
};

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
struct TrackChunk<T> {
    track_i: usize,
    src_i: usize,
    chunk: Vec<T>, // TODO: slice
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
struct PatternMatch {
    chunk_i: usize,
    start: usize,
    length: usize,
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
struct TrackPattern {
    src_i: usize,
    pattern_i: usize,
}

// Very naive algorithm to find matching subsections within all chunks
// Has risk of exploding as the amount of chunks grows, but hopefully it will stay usable
fn crunchy_chunky<T: PartialEq + Debug + Clone>(
    chunks: &[TrackChunk<T>],
    dict_overhead: usize,
) -> Option<Vec<PatternMatch>> {
    let matches_save = |m: &Vec<PatternMatch>| ((m[0].length - 1) * m.len()) as isize - dict_overhead as isize;

    // [ci][ei] => (discovered)
    let mut chunk_lookups: Vec<_> = chunks
        .iter()
        .map(|c| vec![HashSet::<usize>::new(); c.chunk.len()])
        .collect();

    let mut match_sets: Vec<Vec<PatternMatch>> = vec![];
    for outer in chunks {
        for outer_ei in 0..outer.chunk.len() {
            let mut contenders = HashMap::<usize, Vec<PatternMatch>>::new();
            for (inner_i, inner) in chunks.iter().enumerate() {
                for inner_ei in 0..inner.chunk.len() {
                    let max_length = min(inner.chunk.len() - inner_ei, outer.chunk.len() - outer_ei);
                    for match_i in 0..max_length {
                        if inner.chunk[inner_ei + match_i] == outer.chunk[outer_ei + match_i] {
                            let length = match_i + 1;
                            if !chunk_lookups[inner_i][inner_ei].contains(&length) {
                                let dst_key = PatternMatch {
                                    chunk_i: inner_i,
                                    start: inner_ei,
                                    length,
                                };
                                if let Some(v) = contenders.get_mut(&length) {
                                    v.push(dst_key.clone());
                                } else {
                                    contenders.insert(length, vec![dst_key.clone()]);
                                }
                                chunk_lookups[inner_i][inner_ei].insert(length);
                            }
                        } else {
                            break;
                        }
                    }
                }
            }
            for match_set in contenders.into_values() {
                // require at least one match other than the source, and saving at least 1 byte
                if match_set.len() >= 2 && matches_save(&match_set) >= 1 {
                    match_sets.push(match_set);
                }
            }
        }
    }

    // remove for overlappers
    for match_set in &mut match_sets {
        match_set.sort_by_key(|c| c.start);
        let mut last_ends = HashMap::<usize, usize>::new();
        // backwards
        for match_i in (0..match_set.len()).rev() {
            let mach = match_set[match_i].clone();
            if let Some(v) = last_ends.get_mut(&mach.chunk_i) {
                if mach.start + mach.length > match_set[*v].start {
                    match_set[*v].length = 0;
                    *v = match_i;
                } else {
                    *v = match_i;
                }
            } else {
                last_ends.insert(mach.chunk_i, match_i);
            }
        }
        for match_i in (0..match_set.len()).rev() {
            if match_set[match_i].length == 0 {
                match_set.swap_remove(match_i);
            }
        }
    }

    // Sort by size saved
    match_sets.sort_unstable_by_key(|m| -matches_save(m));

    if match_sets.is_empty() {
        None
    } else {
        // Info
        /*
        println!("Best crunchables ({} total)", contenders.len());
        for matches in contenders.iter().take(5) {
            let m0 = &matches[0];
            println!(
                "-> chunk:{}, track:{}, src:{}, length:{} matches:{}, total size:{}, saves:{}",
                m0.chunk_i,
                chunks[m0.chunk_i].track_i,
                m0.start,
                m0.length,
                matches.len(),
                matches.len() * m0.length,
                matches_save(&matches),
            );
            for mach in matches {
                println!(
                    "  -> chunk:{}, track:{}, src:{}",
                    mach.chunk_i, chunks[mach.chunk_i].track_i, mach.start,
                );
            }
        }
        */
        Some(match_sets.swap_remove(0))
    }
}

pub fn crunch<T: PartialEq + Clone + Debug>(
    tracks: Vec<Vec<T>>,
    dict_max: usize,
    dict_overhead: usize,
) -> (Vec<Vec<T>>, Vec<Vec<usize>>) {
    let track_count = tracks.len();
    // convert all tracks to chunks
    let mut chunks: Vec<_> = tracks
        .into_iter()
        .enumerate()
        .map(|(i, t)| TrackChunk {
            track_i: i,
            src_i: 0,
            chunk: t,
        })
        .collect();

    let mut dict: Vec<Vec<T>> = vec![];
    let mut dict_uses: Vec<Vec<TrackPattern>> = vec![vec![]; track_count];

    // find chunks and split up until we need the remaining patterns for the leftover chunks
    // TODO: checking chunks.len() here is likely not enough, because the operation we decide to do could explode the amount of chunks
    // For now * 2 as to not max out
    while dict.len() + chunks.len() * 2 < dict_max {
        //println!("crunching pattern {} | chunk count: {}...", dict.len(), chunks.len());

        // find best chunk matches
        if let Some(matches) = crunchy_chunky(&chunks, dict_overhead) {
            // create pattern
            let pattern = {
                let m0 = &matches[0];
                chunks[m0.chunk_i].chunk[m0.start..m0.start + m0.length].to_vec()
            };
            let pattern_i = dict.len();
            dict.push(pattern.clone());

            // group based the chunk they belong in
            let matches_per_chunk: Vec<(usize, Vec<PatternMatch>)> = matches
                .into_iter()
                .fold(HashMap::<usize, Vec<PatternMatch>>::new(), |mut a, m| {
                    if let Some(cs) = a.get_mut(&m.chunk_i) {
                        cs.push(m);
                    } else {
                        a.insert(m.chunk_i, vec![m]);
                    }
                    a
                })
                .into_iter()
                .collect();

            // split the chunks
            // we replace each modified chunk with an empty chunk as to not mess up ordering
            // all new chunks are placed at the end
            for (chunk_i, mut matches_for_chunk) in matches_per_chunk {
                matches_for_chunk.sort_by_key(|c| c.start); // ordered
                let track_i = chunks[chunk_i].track_i;
                let chunk_src_i = chunks[chunk_i].src_i;
                let mut last_cut: usize = 0;
                for mtch in &matches_for_chunk {
                    if mtch.start > last_cut {
                        chunks.push(TrackChunk {
                            track_i,
                            src_i: chunk_src_i + last_cut,
                            chunk: chunks[chunk_i].chunk[last_cut..mtch.start].to_vec(),
                        });
                    }
                    dict_uses[track_i].push(TrackPattern {
                        src_i: chunk_src_i + mtch.start,
                        pattern_i,
                    });
                    last_cut = mtch.start + mtch.length;
                }
                chunks.push(TrackChunk {
                    track_i,
                    src_i: chunk_src_i + last_cut,
                    chunk: chunks[chunk_i].chunk[last_cut..].to_vec(),
                });
                chunks[chunk_i] = TrackChunk {
                    track_i: 0,
                    src_i: 0,
                    chunk: vec![],
                };
            }

            // delete empty chunks
            let mut i: usize = 0;
            while i < chunks.len() {
                while i < chunks.len() && chunks[i].chunk.is_empty() {
                    chunks.swap_remove(i);
                }
                i += 1;
            }
        } else {
            break;
        }
        //println!("after:\n -> {:?}\n -> {:?}\n -> {:?} ", dict, chunks, dict_uses);
    }

    // convert remaining chunks into doct
    for chunk in chunks {
        let pattern_i = dict.len();
        dict.push(chunk.chunk);
        dict_uses[chunk.track_i].push(TrackPattern {
            src_i: chunk.src_i,
            pattern_i,
        });
    }

    // finalize
    let usages: Vec<Vec<usize>> = dict_uses
        .into_iter()
        .map(|mut ct| {
            ct.sort_by_key(|t| t.src_i);
            ct.into_iter().map(|c| c.pattern_i).collect()
        })
        .collect();

    (dict, usages)
}

pub fn uncrunch<T: Clone>(dict: &[Vec<T>], tracks: &[Vec<usize>]) -> Vec<Vec<T>> {
    tracks
        .iter()
        .map(|t| t.iter().flat_map(|i| dict[*i].clone()).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use crate::crunch::*;

    #[test]
    fn test_uncrunch() {
        assert_eq!(
            vec![vec![1, 1, 1, 0, 0, 0], vec![0, 0, 0, 1, 1, 1]],
            uncrunch(&vec![vec![0, 0, 0], vec![1, 1, 1]], &vec![vec![1, 0], vec![0, 1]])
        );
    }

    #[test]
    fn test_crunch() {
        let input = vec![vec![9, 0, 0, 1, 1, 2, 0], vec![2, 2, 0, 0, 1, 1, 4]];
        let (dict, usages) = crunch(input.clone(), 99, 1);
        assert_eq!(uncrunch(&dict, &usages), input);
    }

    #[test]
    fn test_crunch_bug1() {
        let input = vec![vec![123, 123, 34, 1234, 123, 123], vec![34, 1234]];
        let (dict, usages) = crunch(input.clone(), 99, 1);
        assert_eq!(input, uncrunch(&dict, &usages));
    }

    #[test]
    fn test_crunch_bug2() {
        let input = vec![vec![1, 1, 1]];
        let (dict, usages) = crunch(input.clone(), 99, 1);
        assert_eq!(input, uncrunch(&dict, &usages));
    }

    #[test]
    fn test_crunch_bug3() {
        let input: Vec<Vec<u8>> = vec![vec![5, 4, 4, 1, 1, 1, 6, 6, 7, 1, 1, 0, 4, 4, 4, 3]];
        let (dict, usages) = crunch(input.clone(), 99, 1);
        assert_eq!(input, uncrunch(&dict, &usages));
    }

    #[test]
    fn test_crunch_bug4() {
        for _ in 0..128 {
            let input: Vec<Vec<u8>> = vec![
                vec![1, 2, 0, 2, 4, 1, 7, 1, 5, 0, 2, 4, 6, 2, 2, 4],
                vec![3, 1, 4, 5, 0, 6, 6, 2, 2, 6, 6, 7, 3, 3, 0, 5],
            ];
            println!("{:?}", input);
            let (dict, usages) = crunch(input.clone(), 99, 1);
            assert_eq!(input, uncrunch(&dict, &usages));
        }
    }

    #[test]
    fn test_crunch_fuzz() {
        for _ in 0..128 {
            let buf: Vec<u8> = (0..16).map(|_| rand::thread_rng().gen_range(0..8)).collect();
            println!("{:?}", buf);
            let input = vec![buf];
            let (dict, usages) = crunch(input.clone(), 99, 1);
            assert_eq!(input, uncrunch(&dict, &usages));
        }
    }

    #[test]
    fn test_crunch_deterministic() {
        let buf: Vec<u8> = (0..16).map(|_| rand::thread_rng().gen_range(0..8)).collect();
        println!("{:?}", buf);
        let input = vec![buf];
        let (dict_1, usages_1) = crunch(input.clone(), 16, 1);
        assert_eq!(input, uncrunch(&dict_1, &usages_1));
        for _ in 0..32 {
            let (dict_2, usages_2) = crunch(input.clone(), 16, 1);
            assert_eq!(dict_1, dict_2);
            assert_eq!(usages_1, usages_2);
        }
    }
}
