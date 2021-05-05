mkdir library1 library2 application1 application2 doc

cd library1
cat > lib1.h <<EOF
int life()
{
  return 41;
}
EOF
git add .
git commit -m "Add library1"

cd ../application1
cat > app.c <<EOF
#include "lib1.h"
void main(void)
{
  printf("Answer to life: %d\n", life());
}
EOF
git add .
git commit -m "Add application1"

cd ../library2
cat > lib2.h <<EOF
int universe()
{
  return 42;
}
int everything()
{
  return 42;
}
EOF
git add .
git commit -m "Add library2"

cd ../application2
cat > guide.c <<EOF
#include "lib1.h"
#include "lib2.h"
void main(void)
{
  printf("Answer to life, the universe, and everyting: %d, %d, %d\n", life(), universe(), everything());
}
EOF
git add .
git commit -m "Add application2"

cd ../doc
cat > library1.md <<EOF
Library1 provides the answer to life in an easily digestible packaging
to include in all your projects
EOF
cat > library2.md <<EOF
Library2 provides the answer to the universe and everything
EOF
cat > guide.md <<EOF
The guide project aimes to adress matters of life, universe, and everything.
EOF
git add .
git commit -m "Add documentation"

cd ..
