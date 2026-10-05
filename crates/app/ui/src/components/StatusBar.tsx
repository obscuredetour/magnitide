import { For, Show } from 'solid-js';
import Check from 'lucide-solid/icons/check';
import CircleAlert from 'lucide-solid/icons/circle-alert';
import Clock from 'lucide-solid/icons/clock';
import LoaderCircle from 'lucide-solid/icons/loader-circle';

import { shownItems } from '../devices';
import { clearJobs, type Job, jobs, markJobsSeen, runningJob, unseenFailures } from '../jobs';
import { library } from '../library';
import { Icon } from './Icon';
import { Menu, MenuItem, MenuLabel, MenuSeparator } from './Menu';

/** Detail lines shown per job; the rest are counted. */
const DETAIL_LINES = 20;

/** The bar along the bottom: how many books are shown, what's running, and this session's activity. */
export function StatusBar() {
  const count = () => {
    const total = library()?.bookCount ?? 0;
    const shown = shownItems().length;
    const books = total === 1 ? 'book' : 'books';
    return shown === total
      ? `${total.toLocaleString()} ${books}`
      : `${shown.toLocaleString()} of ${total.toLocaleString()} ${books}`;
  };
  return (
    <footer class="fixed inset-x-0 bottom-0 z-10 flex h-(--statusbar-h) items-center gap-3 border-t border-line bg-bg pr-2 pl-4 text-xs text-muted">
      <span class="whitespace-nowrap tabular-nums">{count()}</span>
      <div class="ml-auto flex min-w-0 items-center gap-2">
        <Show when={runningJob()}>
          {(job) => (
            <span class="flex min-w-0 items-center gap-1.5" role="status">
              <Icon of={LoaderCircle} size={12} class="animate-spin" />
              <span class="truncate">{job().status}</span>
            </span>
          )}
        </Show>
        <ActivityMenu />
      </div>
    </footer>
  );
}

function ActivityMenu() {
  const finished = () => jobs().filter((j) => j.state !== 'running').length;
  return (
    <Menu
      label="Activity"
      iconOnly
      align="end"
      onOpen={markJobsSeen}
      buttonClass="flex h-5 cursor-pointer items-center gap-1 rounded-control px-1.5 hover:bg-hover hover:text-fg aria-expanded:bg-hover"
      panelClass="w-96 top-auto! bottom-full mb-1.5"
      button={
        <>
          <Icon of={Clock} size={12} />
          <Show when={unseenFailures()}>
            <span class="rounded-chip bg-danger px-1 text-[10px] font-semibold text-on-status tabular-nums">
              {unseenFailures()}
            </span>
          </Show>
        </>
      }
    >
      <MenuLabel>This session</MenuLabel>
      <div class="max-h-[min(420px,60vh)] overflow-y-auto select-text">
        <For
          each={jobs()}
          fallback={
            <p class="px-2.5 py-1.5 text-xs text-muted">
              Nothing yet. Sending, converting, adding and deleting books are listed here until you quit.
            </p>
          }
        >
          {(job) => <JobRow job={job} />}
        </For>
      </div>
      <Show when={finished()}>
        <MenuSeparator />
        <MenuItem onSelect={clearJobs}>Clear History</MenuItem>
      </Show>
    </Menu>
  );
}

function JobRow(props: { job: Job }) {
  const j = () => props.job;
  const time = () => j().started.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  const more = () => j().details.length - DETAIL_LINES;
  return (
    <div class="flex gap-2 px-2.5 py-1.5">
      <span
        class="mt-px"
        classList={{
          'text-danger': j().state === 'failed',
          'text-success': j().state === 'done',
          'text-muted': j().state === 'running',
        }}
      >
        <Show when={j().state === 'running'} fallback={<Icon of={j().state === 'failed' ? CircleAlert : Check} />}>
          <Icon of={LoaderCircle} class="animate-spin" />
        </Show>
      </span>
      <div class="min-w-0 flex-1">
        <div class="flex items-baseline justify-between gap-2">
          <span class="truncate font-medium">{j().title}</span>
          <span class="flex-none text-[11px] text-faint tabular-nums">{time()}</span>
        </div>
        <p class="text-xs break-words text-muted">{j().status}</p>
        <Show when={j().details.length}>
          <ul class="mt-1 space-y-0.5 text-[11px] break-words text-faint">
            <For each={j().details.slice(0, DETAIL_LINES)}>{(line) => <li>{line}</li>}</For>
            <Show when={more() > 0}>
              <li>and {more().toLocaleString()} more</li>
            </Show>
          </ul>
        </Show>
      </div>
    </div>
  );
}
