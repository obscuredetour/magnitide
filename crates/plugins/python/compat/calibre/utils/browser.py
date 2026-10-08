"""Mirrors calibre's `calibre.utils.browser`: a mechanize browser with the
conveniences calibre adds (`open_novisit`, `clone_browser`, cookies,
`set_user_agent`, a retry on `Connection reset`)."""

import copy
import ssl
from http.cookiejar import Cookie

import mechanize


def copy_cookiejar(jar):
    """A deep copy of a cookie jar. mechanize's jar leaves its lock out of
    its pickled state and doesn't put one back, so a copy would fail on
    first use."""
    import threading

    ans = copy.deepcopy(jar) if jar is not None else mechanize.CookieJar()
    if not hasattr(ans, '_cookies_lock'):
        ans._cookies_lock = threading.RLock()
    return ans


class ModernBrowser(mechanize.Browser):
    pass


class Browser(ModernBrowser):
    handler_classes = mechanize.Browser.handler_classes.copy()

    def __init__(self, *args, **kwargs):
        self._clone_actions = {}
        self.https_handler = None
        verify_ssl = kwargs.pop('verify_ssl', True)
        if not verify_ssl:
            ctx = ssl.create_default_context()
            ctx.check_hostname = False
            ctx.verify_mode = ssl.CERT_NONE
            kwargs['https_handler'] = mechanize.HTTPSHandler(context=ctx)
        mechanize.Browser.__init__(self, *args, **kwargs)
        self.set_cookiejar(mechanize.CookieJar())

    def set_handle_refresh(self, *args, **kwargs):
        mechanize.Browser.set_handle_refresh(self, *args, **kwargs)
        self._clone_actions['set_handle_refresh'] = ('set_handle_refresh', args, kwargs)

    def set_handle_redirect(self, *args, **kwargs):
        mechanize.Browser.set_handle_redirect(self, *args, **kwargs)
        self._clone_actions['set_handle_redirect'] = ('set_handle_redirect', args, kwargs)

    def set_handle_equiv(self, *args, **kwargs):
        mechanize.Browser.set_handle_equiv(self, *args, **kwargs)
        self._clone_actions['set_handle_equiv'] = ('set_handle_equiv', args, kwargs)

    def set_handle_gzip(self, handle):
        mechanize.Browser._set_handler(self, '_gzip', handle)
        self._clone_actions['set_handle_gzip'] = ('set_handle_gzip', (handle,), {})

    def set_debug_redirect(self, *args, **kwargs):
        mechanize.Browser.set_debug_redirect(self, *args, **kwargs)
        self._clone_actions['set_debug_redirect'] = ('set_debug_redirect', args, kwargs)

    def set_debug_responses(self, *args, **kwargs):
        mechanize.Browser.set_debug_responses(self, *args, **kwargs)
        self._clone_actions['set_debug_responses'] = ('set_debug_responses', args, kwargs)

    def set_debug_http(self, *args, **kwargs):
        mechanize.Browser.set_debug_http(self, *args, **kwargs)
        self._clone_actions['set_debug_http'] = ('set_debug_http', args, kwargs)

    def set_handle_robots(self, *args, **kwargs):
        mechanize.Browser.set_handle_robots(self, *args, **kwargs)
        self._clone_actions['set_handle_robots'] = ('set_handle_robots', args, kwargs)

    def set_proxies(self, *args, **kwargs):
        mechanize.Browser.set_proxies(self, *args, **kwargs)
        self._clone_actions['set_proxies'] = ('set_proxies', args, kwargs)

    def add_password(self, *args, **kwargs):
        mechanize.Browser.add_password(self, *args, **kwargs)
        self._clone_actions['add_password'] = ('add_password', args, kwargs)

    def add_proxy_password(self, *args, **kwargs):
        mechanize.Browser.add_proxy_password(self, *args, **kwargs)
        self._clone_actions['add_proxy_password'] = ('add_proxy_password', args, kwargs)

    def set_user_agent(self, ua):
        self.addheaders = [(k, v) for k, v in self.addheaders if k.lower() != 'user-agent']
        self.addheaders.append(('User-agent', ua))

    @property
    def user_agent(self):
        for k, v in self.addheaders:
            if k.lower() == 'user-agent':
                return v
        return ''

    @user_agent.setter
    def user_agent(self, val):
        self.set_user_agent(val)

    def clone_browser(self):
        clone = self.__class__()
        clone.https_handler = self.https_handler
        clone.addheaders = copy.deepcopy(self.addheaders)
        clone.set_cookiejar(copy_cookiejar(self.cookiejar) if hasattr(self, 'cookiejar') else mechanize.CookieJar())
        for func, args, kwargs in self._clone_actions.values():
            func = getattr(clone, func)
            func(*args, **kwargs)
        return clone

    def set_cookie(self, name, value, domain, path='/'):
        self.cookiejar.set_cookie(
            Cookie(
                None,
                name,
                value,
                None,
                False,
                domain,
                True,
                domain.startswith('.'),
                path,
                True,
                False,
                None,
                False,
                None,
                None,
                None,
            )
        )

    def set_simple_cookie(self, name, value, domain, path='/'):
        self.set_cookie(name, value, domain, path)

    def open_novisit(self, *args, **kwargs):
        """Open a URL without adding it to the browser's history."""
        target = args[0] if args else kwargs.get('url')
        try:
            return self.open(*args, **kwargs)
        except Exception as e:
            if 'Connection reset' in str(e) or 'reset by peer' in str(e):
                return self.open(*args, **kwargs)
            raise
        finally:
            del target

    @property
    def cookiejar(self):
        return getattr(self, '_ua_handlers', {}).get('_cookies', None) and self._ua_handlers['_cookies'].cookiejar

    def set_cookiejar(self, cj):
        mechanize.Browser.set_cookiejar(self, cj)

    def set_current_header(self, header, value=None):
        found = False
        q = header.lower()
        remove = []
        for i, (k, v) in enumerate(self.addheaders):
            if k.lower() == q:
                if value:
                    self.addheaders[i] = (header, value)
                    found = True
                else:
                    remove.append(i)
        for i in reversed(remove):
            del self.addheaders[i]
        if not found and value:
            self.addheaders.append((header, value))

    def set_user_agent_from_js_browser(self, *a, **k):
        pass

    def copy_cookies_from_jsbrowser(self, *a, **k):
        pass
