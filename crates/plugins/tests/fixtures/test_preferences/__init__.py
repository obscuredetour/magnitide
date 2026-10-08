"""A preferences plugin (a page in calibre's Preferences) for Magnitide's
plugin host tests; its widget asks for a restart on commit."""

from calibre.customize import PreferencesPlugin


class TestPreferences(PreferencesPlugin):
    name = 'Magnitide Test Preferences'
    description = 'A settings page'
    author = 'Magnitide'
    version = (1, 0, 0)
    minimum_calibre_version = (5, 0, 0)
    supported_platforms = ['windows', 'osx', 'linux']
    config_widget = 'calibre_plugins.magnitide_test_preferences.widget'
    category = 'Plugins'
    gui_category = 'Plugins'
    gui_name = 'Test preferences'
