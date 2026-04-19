def generate_test_nodes():
    node_filters = []

    for subdir in tree.dirs("tests"):
        is_experimental = subdir == "tests/experimental"
        category = subdir.split("/")[1]

        test_files = [
            filter.file(f)
            for f in tree.files(subdir)
            if f.endswith(".t")
        ]

        if not test_files:
            continue

        helper_files = [
            filter.file(f)
            for s in tree.dirs("tests")
            for f in tree.files(s)
            if not f.endswith(".t")
        ]

        worktree = compose([
            filter.rename("run.sh", "ws/tests.sh"),
            filter.file("run-tests.sh"),
            filter.subdir("scripts").prefix("scripts"),
        ] + test_files + helper_files).prefix("worktree")

        inputs = compose([
            filter.treeid("josh", filter.stored("ws/build-rust")),
            filter.treeid("build-go", filter.stored("ws/build-go")),
        ]).prefix("inputs")

        metadata = compose([
            filter.blob("label", category),
            filter.blob("output", "workdir"),
            filter.treeid("image", filter.stored("images/dev-local")),
        ])

        if is_experimental:
            env = filter.blob("JOSH_EXPERIMENTAL_FEATURES", "1").prefix("env")
            test_node = compose([metadata, inputs, worktree, env])
        else:
            test_node = compose([metadata, inputs, worktree])

        node_filters.append(filter.treeid(category, test_node))

    return compose(node_filters)

filter = generate_test_nodes()
