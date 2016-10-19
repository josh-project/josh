import unittest
import commands
from commands import getoutput
from commands import getstatusoutput

def getoutput(x):
    print "call:", x
    o = commands.getoutput(x)
    print "output:", o
    return o


class CentralGitTests(unittest.TestCase):
    def setUp(self):
        print "setUp"
        getoutput("docker stop centralgit")
        getoutput("docker wait centralgit")
        for n in getoutput("docker ps --no-trunc -aq").split("\n"):
            getoutput("docker rm {}".format(n))
        getoutput("docker run --name centralgit -p 2222:22 -d centralgit/centralgit")

        # getoutput("ssh ssh://git@localhost:2222 cg delete test.git")
        # getoutput("ssh ssh://git@localhost:2222 cg create test.git")
        getoutput("rm -Rf _data*")
        getoutput("mkdir _data0")
        getoutput("mkdir _data1")

    def tearDown(self):
        getoutput("ssh -p 2222 git@localhost cg log")
        getoutput("docker stop centralgit")
        getoutput("docker wait centralgit")
        for n in getoutput("docker ps --no-trunc -aq").split("\n"):
            getoutput("docker rm {}".format(n))

    def test_clone_all(self):
        getoutput("cd _data0 && git clone ssh://git@localhost:2222/test.git")
        getoutput("cd _data0/test && echo bla > foo")
        getoutput("cd _data0/test && git add foo")
        getoutput("cd _data0/test && git commit -m add_foo")
        getoutput("cd _data0/test && git push")

        getoutput("cd _data1 && git clone ssh://git@localhost:2222/test.git")
        self.assertEquals("bla", getoutput("cd _data1/test && cat foo"))
        getoutput("tree")


    def test_clone_sub(self):
        getoutput("cd _data0 && git clone ssh://git@localhost:2222/test.git")
        getoutput("cd _data0/test && mkdir sub && echo sub_bla > sub/foo")
        getoutput("cd _data0/test && git add sub")
        getoutput("cd _data0/test && git commit -m add_sub_foo")
        getoutput("cd _data0/test && git push")

        getoutput("cd _data1 && git clone ssh://git@localhost:2222/test.git")
        self.assertEquals("sub_bla", getoutput("cd _data1/test && cat sub/foo"))

        getoutput("cd _data1 && git clone ssh://git@localhost:2222/test.git/sub")
        self.assertEquals("sub_bla", getoutput("cd _data1/sub && cat foo"))
        getoutput("tree")


if __name__ == "__main__":
    getoutput("cp ~/.ssh/id_rsa.pub ../id_rsa.pub")
    status, output = getstatusoutput("cd .. && cargo test")
    print output
    if status == 0:
        print getoutput("cd .. && docker build -t centralgit/centralgit .")
        unittest.main()
