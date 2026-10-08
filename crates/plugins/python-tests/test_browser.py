"""calibre.browser(): mechanize with calibre's defaults."""


def test_cloned_browser_has_a_working_cookie_jar():
    from calibre import browser

    br = browser(user_agent='magnitide-tests')
    br.set_cookie('session', '1', 'example.com')
    clone = br.clone_browser()
    assert clone.cookiejar is not br.cookiejar
    assert hasattr(clone.cookiejar, '_cookies_lock'), "mechanize's copy loses its lock"
    # The lock is used on every cookie operation.
    clone.set_cookie('other', '2', 'example.com')
    names = sorted(c.name for c in clone.cookiejar)
    assert names == ['other', 'session']
    assert sorted(c.name for c in br.cookiejar) == ['session'], 'the original is untouched'
    assert 'magnitide-tests' in dict(clone.addheaders).get('User-agent', '')
