cd ${TESTTMP}
git init 1> /dev/null

mkdir a
echo "cws = :/c" > a/workspace.josh
echo contents1 > a/file_a2
git add a

mkdir b
echo contents1 > b/file_b1
git add b

mkdir -p c/d
echo contents1 > c/d/file_cd
git add c
git commit -m "add dirs" 1> /dev/null

echo contents2 > c/d/file_cd2
git add c
git commit -m "add file_cd2" 1> /dev/null

mkdir -p c/d/e
echo contents2 > c/d/e/file_cd3
git add c
git commit -m "add file_cd3" 1> /dev/null

echo contents3 >> c/d/e/file_cd3
git add c
git commit -m "edit file_cd3" 1> /dev/null
