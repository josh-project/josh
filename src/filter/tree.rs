use super::*;

#[cfg(feature = "search")]
use rayon::prelude::*;

pub fn pathstree<'a>(
    root: &str,
    input: git2::Oid,
    transaction: &'a cache::Transaction,
) -> JoshResult<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_paths((input, root.to_string())) {
        return Ok(repo.find_tree(cached)?);
    }

    let tree = repo.find_tree(input)?;
    let mut result = empty(repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| josh_error("no name"))?;
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let path = normalize_path(&Path::new(root).join(name));
            let path_string = path.to_str().ok_or_else(|| josh_error("no name"))?;
            let file_contents = if name == "workspace.josh" {
                format!(
                    "#{}\n{}",
                    path_string,
                    get_blob(repo, &tree, Path::new(&name))
                )
            } else {
                path_string.to_string()
            };
            result = replace_child(
                repo,
                Path::new(name),
                repo.blob(file_contents.as_bytes())?,
                0o0100644,
                &result,
            )?;
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = pathstree(
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                entry.id(),
                transaction,
            )?
            .id();

            if s != empty_id() {
                result = replace_child(repo, Path::new(name), s, 0o0040000, &result)?;
            }
        }
    }
    transaction.insert_paths((input, root.to_string()), result.id());
    Ok(result)
}

pub fn regex_replace<'a>(
    input: git2::Oid,
    regex: &regex::Regex,
    replacement: &str,
    transaction: &'a cache::Transaction,
) -> super::JoshResult<git2::Tree<'a>> {
    let repo = transaction.repo();

    let tree = repo.find_tree(input)?;
    let mut result = tree::empty(repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or(super::josh_error("no name"))?;
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let file_contents = get_blob(repo, &tree, std::path::Path::new(&name));
            let replaced = regex.replacen(&file_contents, 0, replacement);

            result = replace_child(
                repo,
                std::path::Path::new(name),
                repo.blob(replaced.as_bytes())?,
                entry.filemode(),
                &result,
            )?;
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = regex_replace(entry.id(), regex, replacement, transaction)?.id();

            if s != tree::empty_id() {
                result = replace_child(repo, std::path::Path::new(name), s, 0o0040000, &result)?;
            }
        }
    }
    Ok(result)
}

pub fn remove_pred<'a>(
    transaction: &'a cache::Transaction,
    root: &str,
    input: git2::Oid,
    pred: &dyn Fn(&Path, bool) -> bool,
    key: git2::Oid,
) -> JoshResult<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_glob((input, key)) {
        return Ok(repo.find_tree(cached)?);
    }
    rs_tracing::trace_scoped!("remove_pred X", "root": root);

    let tree = repo.find_tree(input)?;
    let mut result = empty(repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| josh_error("INVALID_FILENAME"))?;
        let path = std::path::PathBuf::from(root).join(name);

        if entry.kind() == Some(git2::ObjectType::Blob) && pred(&path, true) {
            result = replace_child(
                repo,
                Path::new(entry.name().ok_or_else(|| josh_error("no name"))?),
                entry.id(),
                entry.filemode(),
                &result,
            )?;
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = if !root.is_empty() && pred(&path, false) {
                entry.id()
            } else {
                remove_pred(
                    transaction,
                    &format!(
                        "{}{}{}",
                        root,
                        if root.is_empty() { "" } else { "/" },
                        entry.name().ok_or_else(|| josh_error("no name"))?
                    ),
                    entry.id(),
                    &pred,
                    key,
                )?
                .id()
            };

            if s != empty_id() {
                result = replace_child(
                    repo,
                    Path::new(entry.name().ok_or_else(|| josh_error("no name"))?),
                    s,
                    0o0040000,
                    &result,
                )?;
            }
        }
    }

    transaction.insert_glob((input, key), result.id());
    Ok(result)
}

pub fn subtract(
    transaction: &cache::Transaction,
    input1: git2::Oid,
    input2: git2::Oid,
) -> JoshResult<git2::Oid> {
    let repo = transaction.repo();
    if input1 == input2 {
        return Ok(empty_id());
    }
    if input1 == empty_id() {
        return Ok(empty_id());
    }

    if let Some(cached) = transaction.get_subtract((input1, input2)) {
        return Ok(cached);
    }

    if let (Ok(tree1), Ok(tree2)) = (repo.find_tree(input1), repo.find_tree(input2)) {
        if input2 == empty_id() {
            return Ok(input1);
        }
        rs_tracing::trace_scoped!("subtract fast");
        let mut result_tree = tree1.clone();

        for entry in tree2.iter() {
            if let Some(e) = tree1.get_name(entry.name().ok_or_else(|| josh_error("no name"))?) {
                result_tree = replace_child(
                    repo,
                    Path::new(entry.name().ok_or_else(|| josh_error("no name"))?),
                    subtract(transaction, e.id(), entry.id())?,
                    e.filemode(),
                    &result_tree,
                )?;
            }
        }

        transaction.insert_subtract((input1, input2), result_tree.id());

        return Ok(result_tree.id());
    }

    transaction.insert_subtract((input1, input2), empty_id());

    Ok(empty_id())
}

fn replace_child<'a>(
    repo: &'a git2::Repository,
    child: &Path,
    oid: git2::Oid,
    mode: i32,
    full_tree: &git2::Tree,
) -> JoshResult<git2::Tree<'a>> {
    let full_tree_id = {
        let mut builder = repo.treebuilder(Some(full_tree))?;
        if oid == git2::Oid::zero() {
            builder.remove(child).ok();
        } else if oid == empty_id() {
            builder.remove(child).ok();
        } else {
            builder.insert(child, oid, mode).ok();
        }
        builder.write()?
    };
    return Ok(repo.find_tree(full_tree_id)?);
}

pub fn insert<'a>(
    repo: &'a git2::Repository,
    full_tree: &git2::Tree,
    path: &Path,
    oid: git2::Oid,
    mode: i32,
) -> JoshResult<git2::Tree<'a>> {
    if path.components().count() == 1 {
        replace_child(repo, path, oid, mode, full_tree)
    } else {
        let name = Path::new(path.file_name().ok_or_else(|| josh_error("file_name"))?);
        let path = path.parent().ok_or_else(|| josh_error("path.parent"))?;

        let st = if let Ok(st) = full_tree.get_path(path) {
            repo.find_tree(st.id()).unwrap_or(empty(repo))
        } else {
            empty(repo)
        };

        let tree = replace_child(repo, name, oid, mode, &st)?;

        insert(repo, full_tree, path, tree.id(), 0o0040000)
    }
}

pub fn diff_paths(
    repo: &git2::Repository,
    input1: git2::Oid,
    input2: git2::Oid,
    root: &str,
) -> JoshResult<Vec<(String, i32)>> {
    rs_tracing::trace_scoped!("diff_paths");
    if input1 == input2 {
        return Ok(vec![]);
    }

    if let (Ok(_), Ok(_)) = (repo.find_blob(input1), repo.find_blob(input2)) {
        return Ok(vec![(root.to_string(), 0)]);
    }

    if let (Ok(_), Err(_)) = (repo.find_blob(input1), repo.find_blob(input2)) {
        return Ok(vec![(root.to_string(), -1)]);
    }

    if let (Err(_), Ok(_)) = (repo.find_blob(input1), repo.find_blob(input2)) {
        return Ok(vec![(root.to_string(), 1)]);
    }

    let mut r = vec![];

    if let (Ok(tree1), Ok(tree2)) = (repo.find_tree(input1), repo.find_tree(input2)) {
        for entry in tree2.iter() {
            let name = entry.name().ok_or_else(|| josh_error("no name"))?;
            if let Some(e) = tree1.get_name(entry.name().ok_or_else(|| josh_error("no name"))?) {
                r.append(&mut diff_paths(
                    repo,
                    e.id(),
                    entry.id(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            } else {
                r.append(&mut diff_paths(
                    repo,
                    git2::Oid::zero(),
                    entry.id(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            }
        }

        for entry in tree1.iter() {
            let name = entry.name().ok_or_else(|| josh_error("no name"))?;
            if let Some(_) = tree2.get_name(entry.name().ok_or_else(|| josh_error("no name"))?) {
            } else {
                r.append(&mut diff_paths(
                    repo,
                    entry.id(),
                    git2::Oid::zero(),
                    &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                )?);
            }
        }

        return Ok(r);
    }

    if let Ok(tree2) = repo.find_tree(input2) {
        for entry in tree2.iter() {
            let name = entry.name().ok_or_else(|| josh_error("no name"))?;
            r.append(&mut diff_paths(
                repo,
                git2::Oid::zero(),
                entry.id(),
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
            )?);
        }
        return Ok(r);
    }

    if let Ok(tree1) = repo.find_tree(input2) {
        for entry in tree1.iter() {
            let name = entry.name().ok_or_else(|| josh_error("no name"))?;
            r.append(&mut diff_paths(
                repo,
                entry.id(),
                git2::Oid::zero(),
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
            )?);
        }
        return Ok(r);
    }

    Ok(r)
}

pub fn overlay(
    repo: &git2::Repository,
    input1: git2::Oid,
    input2: git2::Oid,
) -> JoshResult<git2::Oid> {
    rs_tracing::trace_scoped!("overlay");
    if input1 == input2 {
        return Ok(input1);
    }
    if input1 == empty_id() {
        return Ok(input2);
    }
    if input2 == empty_id() {
        return Ok(input1);
    }

    if let (Ok(tree1), Ok(tree2)) = (repo.find_tree(input1), repo.find_tree(input2)) {
        let mut result_tree = tree1.clone();

        for entry in tree2.iter() {
            if let Some(e) = tree1.get_name(entry.name().ok_or_else(|| josh_error("no name"))?) {
                result_tree = replace_child(
                    repo,
                    Path::new(entry.name().ok_or_else(|| josh_error("no name"))?),
                    overlay(repo, e.id(), entry.id())?,
                    e.filemode(),
                    &result_tree,
                )?;
            } else {
                result_tree = replace_child(
                    repo,
                    Path::new(entry.name().ok_or_else(|| josh_error("no name"))?),
                    entry.id(),
                    entry.filemode(),
                    &result_tree,
                )?;
            }
        }

        return Ok(result_tree.id());
    }

    Ok(input1)
}

pub fn pathline(b: &str) -> JoshResult<String> {
    for line in b.split('\n') {
        let l = line.trim_start_matches('#');
        if l.is_empty() {
            break;
        }
        return Ok(l.to_string());
    }
    Err(josh_error("pathline"))
}

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
) -> JoshResult<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_trigram_index(tree.id()) {
        return Ok(repo.find_tree(cached)?);
    }

    let mut arrs_own = vec![vec![]; 8];
    let mut arrs_sub = vec![vec![]; 8];

    let mut files = vec![vec![]; 8];

    let mut result = empty(repo);

    /* 'entry: */
    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| josh_error("no name"))?;
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

            for a in 0..arrs_sub.len() {
                let b = get_blob(repo, &s, Path::new(&format!("OWN{}", a)));
                let hd = hex::decode(b.lines().collect::<Vec<_>>().join(""))?;
                let new_size = std::cmp::max(hd.len(), arrs_sub[a].len());
                arrs_sub[a].resize(new_size, 0);
                for (a, b) in arrs_sub[a].iter_mut().zip(hd.iter()) {
                    *a |= b;
                }

                let b = get_blob(repo, &s, Path::new(&format!("SUB{}", a)));
                let hd = hex::decode(b.lines().collect::<Vec<_>>().join(""))?;
                let new_size = std::cmp::max(hd.len(), arrs_sub[a].len());
                arrs_sub[a].resize(new_size, 0);
                for (a, b) in arrs_sub[a].iter_mut().zip(hd.iter()) {
                    *a |= b;
                }
            }

            if s.id() != empty_id() {
                result = replace_child(repo, Path::new(name), s.id(), 0o0040000, &result)?;
            }
        }
    }

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

#[cfg(feature = "search")]
pub fn search_candidates(
    transaction: &cache::Transaction,
    tree: &git2::Tree,
    searchstring: &str,
    max_ord: usize,
) -> JoshResult<Vec<String>> {
    let ff = make_dir_trigram_filter(&searchstring, FILE_FILTER_SIZE, &[2]);

    let mut results = vec![];

    for ord in 0..max_ord {
        let dir_filter_size = usize::pow(4, 3 + ord as u32);
        let df = make_dir_trigram_filter(&searchstring, dir_filter_size, &[0, 1, 2]);
        trigram_search(&transaction, tree.clone(), "", &df, &ff, &mut results, ord)?;
    }
    Ok(results)
}

#[cfg(feature = "search")]
pub fn search_matches(
    transaction: &cache::Transaction,
    tree: &git2::Tree,
    searchstring: &str,
    candidates: &Vec<String>,
) -> JoshResult<Vec<(String, Vec<(usize, String)>)>> {
    let mut results = vec![];

    for c in candidates {
        let b = get_blob(transaction.repo(), tree, &Path::new(&c));

        let mut bresults = vec![];

        for (linenr, l) in b.lines().enumerate() {
            if l.contains(searchstring) {
                bresults.push((linenr + 1, l.to_owned()));
            }
        }

        if bresults.len() != 0 {
            results.push((c.to_owned(), bresults));
        }
    }

    Ok(results)
}

#[cfg(feature = "search")]
pub fn trigram_search<'a>(
    transaction: &'a cache::Transaction,
    tree: git2::Tree<'a>,
    root: &str,
    dir_filter: &[u8],
    file_filter: &[u8],
    results: &mut Vec<String>,
    ord: usize,
) -> JoshResult<()> {
    rs_tracing::trace_scoped!("trigram_search", "ord": ord, "root": root);
    let repo = transaction.repo();

    let hd = {
        rs_tracing::trace_scoped!("get blob own");

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

    let dmatch = if hd.len() != 0 {
        rs_tracing::trace_scoped!("dmatch own");
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
        rs_tracing::trace_scoped!("search blobs");
        let b = get_blob(&repo, &tree, &Path::new(&format!("BLOBS{}", ord)));

        let mut filename = None;
        let mut skip = false;

        for line in b.lines() {
            if line == "" {
                skip = false;
                filename = None;
            } else if filename == None {
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

                if fmatch {
                    if let Some(filename) = filename {
                        results.push(format!(
                            "{}{}{}",
                            root,
                            if root == "" { "" } else { "/" },
                            filename
                        ));
                        skip = true;
                    }
                }
            }
        }
    }

    let hd = {
        rs_tracing::trace_scoped!("get blob sub");

        if let Some(blob) = tree
            .get_name(&format!("SUB{}", ord))
            .map(|x| repo.find_blob(x.id()))
        {
            let blob = blob?;
            let b = unsafe { std::str::from_utf8_unchecked(blob.content()) };
            rs_tracing::trace_scoped!("hex decode sub");
            hex::decode(b.lines().collect::<Vec<_>>().join(""))?
        } else {
            return Ok(());
        }
    };

    {
        rs_tracing::trace_scoped!("dmatch sub");

        for (a, b) in dir_filter.iter().zip(hd.iter()) {
            if a & b != *a {
                return Ok(());
            }
        }
    }

    rs_tracing::trace_scoped!("down par_iter");

    let rpath = transaction.repo().path();

    let trees = tree
        .iter()
        .filter(|x| x.kind() == Some(git2::ObjectType::Tree))
        .filter(|x| x.name().is_some())
        .map(|x| (x.id(), x.name().unwrap().to_string()))
        .collect::<Vec<_>>();

    let mut r = trees
        .par_iter()
        .map_init(
            || cache::Transaction::open(rpath, None).unwrap(),
            |transaction, (id, name)| {
                let s = transaction.repo().find_tree(*id).unwrap();

                let mut results = vec![];

                trigram_search(
                    &transaction,
                    s,
                    &format!("{}{}{}", root, if root == "" { "" } else { "/" }, name),
                    dir_filter,
                    file_filter,
                    &mut results,
                    ord,
                )
                .unwrap();
                results
            },
        )
        .reduce(
            || vec![],
            |mut r, mut b| {
                r.append(&mut b);
                r
            },
        );
    results.append(&mut r);
    Ok(())
}

pub fn invert_paths<'a>(
    transaction: &'a cache::Transaction,
    root: &str,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    let repo = transaction.repo();
    if let Some(cached) = transaction.get_invert((tree.id(), root.to_string())) {
        return Ok(repo.find_tree(cached)?);
    }

    let mut result = empty(repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or_else(|| josh_error("no name"))?;

        if entry.kind() == Some(git2::ObjectType::Blob) {
            let mpath = normalize_path(&Path::new(root).join(name))
                .to_string_lossy()
                .to_string();
            let b = get_blob(repo, &tree, Path::new(name));
            let opath = pathline(&b)?;

            result = insert(
                repo,
                &result,
                Path::new(&opath),
                repo.blob(mpath.as_bytes())?,
                0o0100644,
            )
            .unwrap();
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = invert_paths(
                transaction,
                &format!("{}{}{}", root, if root.is_empty() { "" } else { "/" }, name),
                repo.find_tree(entry.id())?,
            )?;
            result = repo.find_tree(overlay(repo, result.id(), s.id())?)?;
        }
    }

    transaction.insert_invert((tree.id(), root.to_string()), result.id());

    Ok(result)
}

pub fn original_path(
    transaction: &cache::Transaction,
    commit: &git2::Commit<'_>,
    filter: Filter,
    tree: git2::Tree,
    path: &Path,
) -> JoshResult<String> {
    let paths_tree = apply(transaction, commit, chain(to_filter(Op::Paths), filter), tree)?;
    let b = get_blob(transaction.repo(), &paths_tree, path);
    pathline(&b)
}

pub fn repopulated_tree(
    transaction: &cache::Transaction,
    commit: &git2::Commit<'_>,
    filter: Filter,
    full_tree: git2::Tree,
    partial_tree: git2::Tree,
) -> JoshResult<git2::Oid> {
    let paths_tree = apply(transaction, commit, chain(to_filter(Op::Paths), filter), full_tree)?;

    let ipaths = invert_paths(transaction, "", paths_tree)?;
    populate(transaction, ipaths.id(), partial_tree.id())
}

fn populate(
    transaction: &cache::Transaction,
    paths: git2::Oid,
    content: git2::Oid,
) -> JoshResult<git2::Oid> {
    rs_tracing::trace_scoped!("repopulate");

    if let Some(cached) = transaction.get_populate((paths, content)) {
        return Ok(cached);
    }

    let repo = transaction.repo();

    let mut result_tree = empty_id();
    if let (Ok(paths), Ok(content)) = (repo.find_blob(paths), repo.find_blob(content)) {
        let ipath = pathline(std::str::from_utf8(paths.content())?)?;
        result_tree = insert(
            repo,
            &repo.find_tree(result_tree)?,
            Path::new(&ipath),
            content.id(),
            0o0100644,
        )?
        .id();
    } else if let (Ok(paths), Ok(content)) = (repo.find_tree(paths), repo.find_tree(content)) {
        for entry in content.iter() {
            if let Some(e) = paths.get_name(entry.name().ok_or_else(|| josh_error("no name"))?) {
                result_tree = overlay(
                    repo,
                    result_tree,
                    populate(transaction, e.id(), entry.id())?,
                )?;
            }
        }
    }

    transaction.insert_populate((paths, content), result_tree);

    Ok(result_tree)
}

pub fn compose_fast(
    transaction: &cache::Transaction,
    trees: Vec<git2::Oid>,
) -> JoshResult<git2::Tree> {
    rs_tracing::trace_scoped!("compose_fast");
    let repo = transaction.repo();
    let mut result = empty_id();
    for tree in trees {
        result = overlay(repo, tree, result)?;
    }

    Ok(repo.find_tree(result)?)
}

pub fn compose<'a>(
    transaction: &'a cache::Transaction,
    commit: &git2::Commit<'a>,
    trees: Vec<(&Filter, git2::Tree<'a>)>,
) -> JoshResult<git2::Tree<'a>> {
    rs_tracing::trace_scoped!("compose");
    let repo = transaction.repo();
    let mut result = empty(repo);
    let mut taken = empty(repo);
    for (f, applied) in trees {
        let tid = taken.id();
        let taken_applied = if let Some(cached) = transaction.get_apply(*f, tid) {
            cached
        } else {
            apply(transaction, commit, *f, taken.clone())?.id()
        };
        transaction.insert_apply(*f, tid, taken_applied);

        let subtracted = repo.find_tree(subtract(transaction, applied.id(), taken_applied)?)?;

        let aid = applied.id();
        let unapplied = if let Some(cached) = transaction.get_unapply(*f, aid) {
            cached
        } else {
            apply(transaction, commit, invert(*f)?, applied)?.id()
        };
        transaction.insert_unapply(*f, aid, unapplied);
        taken = repo.find_tree(overlay(repo, taken.id(), unapplied)?)?;
        result = repo.find_tree(overlay(repo, subtracted.id(), result.id())?)?;
    }

    Ok(result)
}

pub fn get_blob(repo: &git2::Repository, tree: &git2::Tree, path: &Path) -> String {
    let entry_oid = ok_or!(tree.get_path(path).map(|x| x.id()), {
        return "".to_owned();
    });

    let blob = ok_or!(repo.find_blob(entry_oid), {
        return "".to_owned();
    });

    if blob.is_binary() {
        return "".to_owned();
    }

    let content = ok_or!(std::str::from_utf8(blob.content()), {
        return "".to_owned();
    });

    content.to_owned()
}

pub fn empty_id() -> git2::Oid {
    git2::Oid::from_str("4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap()
}

pub fn empty(repo: &git2::Repository) -> git2::Tree {
    repo.find_tree(empty_id()).unwrap()
}
