  $ export TESTTMP=${PWD}

  $ . ${TESTDIR}/setup_repo.sh

# default permissions give everything
  $ josh-filter -s :/ master --check-permission --update refs/josh/filtered

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  `-- c
      `-- d
          |-- e
          |   `-- file_cd3
          |-- file_cd
          `-- file_cd2
  
  5 directories, 6 files


# default same as this
  $ josh-filter -s :/ master --check-permission -b :empty -w :nop --update refs/josh/filtered_2

  $ git checkout refs/josh/filtered 2> /dev/null
  $ tree
  .
  |-- a
  |   |-- file_a2
  |   `-- workspace.josh
  |-- b
  |   `-- file_b1
  `-- c
      `-- d
          |-- e
          |   `-- file_cd3
          |-- file_cd
          `-- file_cd2
  
  5 directories, 6 files


# no permissions
  $ josh-filter -s :/ master --check-permission -b :nop -w :empty --update refs/josh/filtered
  [3] :INVERT
  [3] :PATHS
  [12] _invert
  [16] _paths
  ERROR: JoshError("missing permissions for ref")
  [1]
  $ josh-filter -s :/b master --check-permission -w ::a/ --update refs/josh/filtered
  [1] :/a
  [1] :/b
  [1] :subtract[
          :/
          ::a/
      ]
  [3] :PATHS
  [4] :INVERT
  [13] _invert
  [16] _paths
  ERROR: JoshError("missing permissions for ref")
  [1]


  $ josh-filter -s :/b master --check-permission -b ::b/ -w ::b/ --update refs/josh/filtered
  [1] :/a
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :prefix=b
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [2] :/b
  [3] :PATHS
  [4] :INVERT
  [13] _invert
  [16] _paths
  ERROR: JoshError("missing permissions for ref")
  [1]


# access granted
  $ josh-filter -s :/b master --check-permission -w ::b/ --update refs/josh/filtered
  [1] :/a
  [1] :[
      ::b/
      :subtract[
              :/
              ::b/
          ]
  ]
  [1] :prefix=b
  [1] :subtract[
          :/
          ::a/
      ]
  [1] :subtract[
          :/
          ::b/
      ]
  [3] :/b
  [3] :PATHS
  [4] :INVERT
  [13] _invert
  [16] _paths


