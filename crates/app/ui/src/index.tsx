import './theme'; // sets data-theme before the first render
import './zoom'; // and the interface size
import './styles.css';

import { render } from 'solid-js/web';

import { App } from './App';

// macOS gets its own title bar (TitleBar), since the window's is an overlay
// with the traffic lights in their usual place.
document.documentElement.classList.toggle('is-mac', /Mac/.test(navigator.userAgent));

render(() => <App />, document.getElementById('root')!);
