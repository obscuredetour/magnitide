"""Mirrors `run_cli` and friends from calibre's `calibre.utils.run_tests`."""

import unittest


def run_cli(suite, verbosity=4, buffer=True):
    r = unittest.TextTestRunner(verbosity=verbosity, buffer=buffer)
    return r.run(suite)


def find_tests_in_package(package, excludes=('main.py',)):
    return unittest.defaultTestLoader.loadTestsFromName(package)


def filter_tests(suite, test_ok):
    ans = unittest.TestSuite()
    for test in (
        unittest.defaultTestLoader.getTestCaseNames(suite.__class__)
        if not isinstance(suite, unittest.TestSuite)
        else suite
    ):
        if test_ok(test):
            ans.addTest(test)
    return ans


__all__ = ['run_cli', 'find_tests_in_package', 'filter_tests']
