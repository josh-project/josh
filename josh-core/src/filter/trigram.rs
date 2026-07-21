use super::tree::{empty_id, get_blob, insert};
use super::*;
use crate::cache::CacheStack;
use crate::cache::TransactionContext;

use anyhow::anyhow;
use std::path::Path;

const FILE_FILTER_SIZE: usize = 64;

#[allow(clippy::many_single_char_names)]
fn hash_bits(s: &str, size: usize) -> [usize; 3] {
    let size = size * 8;
    let size = size / 2;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut hasher = DefaultHasher::new();
    hasher.write(s.as_bytes());
    let r = hasher.finish() as usize;
    let n = 8 * usize::pow(4, 10);

    let (a, b, c) = (r % size, (r / n) % size, ((r / n) / n) % size);

    if s.chars().any(|x| !char::is_alphabetic(x)) {
        [a + size, b + size, c + size]
    } else {
        [a, b, c]
    }
}

pub fn make_dir_trigram_filter(searchstring: &str, size: usize, bits: &[usize]) -> Vec<u8> {
    let mut arr_own = vec![0u8; size];
    let abf = bitvec::slice::BitSlice::<_, bitvec::order::Msb0>::from_slice_mut(&mut arr_own);

    for t in searchstring
        .as_bytes()
        .windows(3)
        .filter_map(|x| std::str::from_utf8(x).ok())
    {
        for bit in bits {
            abf.set(hash_bits(t, size)[*bit], true);
        }
    }

    arr_own.to_vec()
}

pub fn trigram_index<'a>(
    transaction: &'a cache::Transaction,
    tree: git2::Tree<'a>,
) -> anyhow::Result<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_trigram_index(tree.id()) {
        return Ok(repo.find_tree(cached)?);
    }

    let mut arrs_own = vec![vec![]; 8];
    let mut arrs_sub = vec![vec![]; 8];

    let mut files = vec![vec![]; 8];

    // The subtree children go into this builder (written once after the loop); the per-level
    // OWN/SUB/BLOBS index blobs are added to the resulting tree afterward.
    let mut builder = repo.treebuilder(None)?;

    /* 'entry: */
    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| anyhow!("no name"))?;
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let b = get_blob(repo, &tree, Path::new(name));

            let mut file_chunks = vec![name.to_string()];

            let trigrams: Vec<_> = b
                .as_bytes()
                .windows(3)
                .filter_map(|x| std::str::from_utf8(x).ok())
                .collect();

            //let mut histogram = std::collections::HashMap::new();

            //for trigram in trigrams.iter() {
            //    let counter = histogram.entry(trigram).or_insert(0);
            //    *counter += 1;
            //}

            //let mut freq: Vec<_> = histogram.iter().map(|(a,b)| (b,a)).collect();
            //freq.sort();

            //let mut hbf =
            //    bitvec::array::BitArray::<bitvec::order::Msb0, _>::new([0u8; FILE_FILTER_SIZE]);
            //for (_,trigram) in freq.iter().rev() {
            //    hbf.set(hash_bits(trigram)[2] % (FILE_FILTER_SIZE * 8), true);
            //    if hbf.count_ones() > FILE_FILTER_SIZE * 4 {
            //        break;
            //    }
            //}

            //let filefilter = hex::encode(hbf.as_buffer());
            //file_chunks.push(format!("{}", filefilter));

            let mut bf =
                bitvec::array::BitArray::<_, bitvec::order::Msb0>::new([0u8; FILE_FILTER_SIZE]);
            let mut i = 0;
            for trigram in trigrams {
                //if hbf[hash_bits(trigram)[2] % (FILE_FILTER_SIZE * 8)] {
                //    continue;
                //}
                bf.set(hash_bits(trigram, FILE_FILTER_SIZE)[2], true);

                if bf.count_ones() > FILE_FILTER_SIZE * 4 {
                    let filefilter = hex::encode(bf.into_inner());
                    file_chunks.push(format!("{} {:04x}", filefilter, i));
                    i = 0;
                    bf.fill(false);
                }
                i += 1;
            }

            //if bf.count_ones() > 0 {
            let filefilter = hex::encode(bf.into_inner());
            file_chunks.push(format!("{} {:04x}", filefilter, i));
            //}
            file_chunks.push("".to_string());

            'arrsub: for a in 0..arrs_sub.len() {
                let dir_filter_size = usize::pow(4, 3 + a as u32);
                let dtf = make_dir_trigram_filter(&b, dir_filter_size, &[0, 1, 2]);
                let abf = bitvec::slice::BitSlice::<_, bitvec::order::Msb0>::from_slice(&dtf);

                if abf.count_ones() > dir_filter_size / 2 {
                    continue 'arrsub;
                }

                arrs_own[a].resize(dtf.len(), 0);
                for (a, b) in arrs_own[a].iter_mut().zip(dtf.iter()) {
                    *a |= b;
                }

                files[a].append(&mut file_chunks);

                break 'arrsub;
            }
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = trigram_index(transaction, transaction.repo().find_tree(entry.id())?)?;

            for (a, arr_sub) in arrs_sub.iter_mut().enumerate() {
                let b = get_blob(repo, &s, Path::new(&format!("OWN{}", a)));
                let hd = hex::decode(b.lines().collect::<Vec<_>>().join(""))?;
                let new_size = std::cmp::max(hd.len(), arr_sub.len());
                arr_sub.resize(new_size, 0);
                for (a, &b) in arr_sub.iter_mut().zip(hd.iter()) {
                    *a |= b;
                }

                let b = get_blob(repo, &s, Path::new(&format!("SUB{}", a)));
                let hd = hex::decode(b.lines().collect::<Vec<_>>().join(""))?;
                let new_size = std::cmp::max(hd.len(), arr_sub.len());
                arr_sub.resize(new_size, 0);
                for (a, &b) in arr_sub.iter_mut().zip(hd.iter()) {
                    *a |= b;
                }
            }

            if s.id() != empty_id() {
                builder.insert(name, s.id(), 0o0040000).ok();
            }
        }
    }

    let mut result = repo.find_tree(builder.write()?)?;

    for a in 0..arrs_sub.len() {
        if arrs_own[a].iter().any(|x| *x != 0) {
            result = insert(
                repo,
                &result,
                Path::new(&format!("OWN{}", a)),
                repo.blob(
                    arrs_own[a]
                        .chunks(64)
                        .map(hex::encode)
                        .collect::<Vec<_>>()
                        .join("\n")
                        .as_bytes(),
                )?,
                0o0100644,
            )
            .unwrap();
        }
        if arrs_sub[a].iter().any(|x| *x != 0) {
            result = insert(
                repo,
                &result,
                Path::new(&format!("SUB{}", a)),
                repo.blob(
                    arrs_sub[a]
                        .chunks(64)
                        .map(hex::encode)
                        .collect::<Vec<_>>()
                        .join("\n")
                        .as_bytes(),
                )?,
                0o0100644,
            )
            .unwrap();
        }
        if !files[a].is_empty() {
            result = insert(
                repo,
                &result,
                Path::new(&format!("BLOBS{}", a)),
                repo.blob(files[a].join("\n").as_bytes())?,
                0o0100644,
            )
            .unwrap();
        }
    }
    transaction.insert_trigram_index(tree.id(), result.id());
    Ok(result)
}

pub fn search_candidates(
    transaction: &cache::Transaction,
    tree: &git2::Tree,
    searchstring: &str,
    max_ord: usize,
) -> anyhow::Result<Vec<String>> {
    let ff = make_dir_trigram_filter(searchstring, FILE_FILTER_SIZE, &[2]);

    let mut results = vec![];

    for ord in 0..max_ord {
        let dir_filter_size = usize::pow(4, 3 + ord as u32);
        let df = make_dir_trigram_filter(searchstring, dir_filter_size, &[0, 1, 2]);
        trigram_search(transaction, tree.clone(), "", &df, &ff, &mut results, ord)?;
    }
    Ok(results)
}

type SearchMatchesResult = Vec<(String, Vec<(usize, String)>)>;

pub fn search_matches(
    transaction: &cache::Transaction,
    tree: &git2::Tree,
    searchstring: &str,
    candidates: &Vec<String>,
) -> anyhow::Result<SearchMatchesResult> {
    let mut results = vec![];

    for c in candidates {
        let b = get_blob(transaction.repo(), tree, Path::new(&c));

        let mut bresults = vec![];

        for (linenr, l) in b.lines().enumerate() {
            if l.contains(searchstring) {
                bresults.push((linenr + 1, l.to_owned()));
            }
        }

        if !bresults.is_empty() {
            results.push((c.to_owned(), bresults));
        }
    }

    Ok(results)
}

pub fn trigram_search<'a>(
    transaction: &'a cache::Transaction,
    tree: git2::Tree<'a>,
    root: &str,
    dir_filter: &[u8],
    file_filter: &[u8],
    results: &mut Vec<String>,
    ord: usize,
) -> anyhow::Result<()> {
    let repo = transaction.repo();

    let hd = {
        if let Some(blob) = tree
            .get_name(&format!("OWN{}", ord))
            .map(|x| repo.find_blob(x.id()))
        {
            let blob = blob?;
            let b = unsafe { std::str::from_utf8_unchecked(blob.content()) };
            hex::decode(b.lines().collect::<Vec<_>>().join(""))?
        } else {
            vec![]
        }
    };

    let dmatch = if !hd.is_empty() {
        let mut dmatch = true;
        for (a, b) in dir_filter.iter().zip(hd.iter()) {
            if a & b != *a {
                dmatch = false;
                break;
            }
        }
        dmatch
    } else {
        false
    };

    if dmatch {
        let b = get_blob(repo, &tree, Path::new(&format!("BLOBS{}", ord)));

        let mut filename = None;
        let mut skip = false;

        for line in b.lines() {
            if line.is_empty() {
                skip = false;
                filename = None;
            } else if filename.is_none() {
                filename = Some(line);
            } else if !skip {
                let hd = hex::decode(&line[..FILE_FILTER_SIZE * 2])?;

                let mut fmatch = true;
                for (a, b) in file_filter.iter().zip(hd.iter()) {
                    if a & b != *a {
                        fmatch = false;
                        break;
                    }
                }

                if fmatch && let Some(filename) = filename {
                    results.push(format!(
                        "{}{}{}",
                        root,
                        if root.is_empty() { "" } else { "/" },
                        filename
                    ));
                    skip = true;
                }
            }
        }
    }

    let hd = {
        if let Some(blob) = tree
            .get_name(&format!("SUB{}", ord))
            .map(|x| repo.find_blob(x.id()))
        {
            let blob = blob?;
            let b = unsafe { std::str::from_utf8_unchecked(blob.content()) };
            hex::decode(b.lines().collect::<Vec<_>>().join(""))?
        } else {
            return Ok(());
        }
    };

    {
        for (a, b) in dir_filter.iter().zip(hd.iter()) {
            if a & b != *a {
                return Ok(());
            }
        }
    }

    let rpath = transaction.repo().path();

    let trees = tree
        .iter()
        .filter(|x| x.kind() == Some(git2::ObjectType::Tree))
        .filter(|x| x.name().is_some())
        .map(|x| (x.id(), x.name().unwrap().to_string()))
        .collect::<Vec<_>>();

    let tran_context = TransactionContext::new(rpath, CacheStack::default().into());
    let sub_transaction = tran_context.open()?;
    for (id, name) in &trees {
        let s = sub_transaction.repo().find_tree(*id)?;

        trigram_search(
            &sub_transaction,
            s,
            &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
            dir_filter,
            file_filter,
            results,
            ord,
        )?;
    }
    Ok(())
}
