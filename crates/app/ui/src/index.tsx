import { render } from 'solid-js/web';

import { App } from './App';
import './styles.css';

// The macOS overlay title bar needs room for the traffic-light buttons.
document.documentElement.classList.toggle('is-mac', /Mac/.test(navigator.userAgent));

render(() => <App />, document.getElementById('root')!);
