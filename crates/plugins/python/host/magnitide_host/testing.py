"""Hooks for Magnitide's own tests, loaded only when MAGNITIDE_TEST_HOOKS is
set: `test.exec` runs Python in the host (on the main thread), so a test
can, say, accept a plugin's settings dialog as a user would."""

from magnitide_host.rpc import method, on_main


@method('test.exec')
def m_test_exec(params):
    from magnitide_host.gui import ensure_qt

    ensure_qt()
    code = params['code']

    def go():
        ns = {}
        exec(code, ns)  # noqa: S102
        return {'result': repr(ns.get('result'))}

    if params.get('main', True):
        return on_main(go)
    return go()
