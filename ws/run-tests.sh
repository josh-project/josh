set -e

sh run-tests.sh -ivy tests/**/*.t || TESTS_FAILED=$?
cp -R ./tests /out/tests
exit ${TESTS_FAILED:-0}
