"""A GUI (interface action) plugin for Magnitide's plugin host tests. It does
what real plugins do: edits metadata through both of calibre's database APIs,
writes into a book file in place, sets a cover, adds a book, reports through
the status bar and opens a dialog."""

from calibre.customize import InterfaceActionBase


class TestActionBase(InterfaceActionBase):
    name = 'Magnitide Test Action'
    description = 'Changes the selected books in ways real plugins do'
    author = 'Magnitide'
    version = (2, 0, 0)
    minimum_calibre_version = (5, 0, 0)
    supported_platforms = ['windows', 'osx', 'linux']
    actual_plugin = 'calibre_plugins.magnitide_test_action.action:TestAction'
