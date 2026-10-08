"""Mirrors calibre's `calibre.utils.random_ua`: browser user agents for
metadata sources. calibre updates a list from a server; ours is a short
built-in list of current browsers."""

import random

_UAS = [
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0',
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36',
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36',
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:128.0) Gecko/20100101 Firefox/128.0',
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15',
    'Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0',
    'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36',
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0',
]


def all_user_agents():
    return list(_UAS)


def common_user_agents():
    return list(_UAS)


def random_user_agent(choose=None, allow_ie=True):
    if choose is not None:
        return _UAS[choose % len(_UAS)]
    return random.choice(_UAS)


def random_common_chrome_user_agent():
    return random.choice([x for x in _UAS if 'Chrome' in x])


def random_desktop_platform():
    return random.choice(['Windows NT 10.0; Win64; x64', 'Macintosh; Intel Mac OS X 10_15_7', 'X11; Linux x86_64'])


def accept_header_for_ua(ua):
    if 'Firefox/' in ua:
        return 'text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8'
    return 'text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8'


def user_agents_popularity():
    return {}


def random_user_agent_with_platform(**kw):
    return random_user_agent()
