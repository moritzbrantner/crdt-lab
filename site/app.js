import init, { TwoReplicaListLab } from "./pkg/crdt_lab_wasm.js";

const leftList = document.querySelector("#left-list");
const rightList = document.querySelector("#right-list");
const leftOutbox = document.querySelector("#left-outbox");
const rightOutbox = document.querySelector("#right-outbox");
const status = document.querySelector("#convergence-status");
const errorMessage = document.querySelector("#error-message");

let lab;
let view;

function setError(error) {
  if (!error) {
    errorMessage.hidden = true;
    errorMessage.textContent = "";
    return;
  }
  errorMessage.hidden = false;
  errorMessage.textContent = error?.message || String(error);
}

function readView() {
  view = JSON.parse(lab.view_json());
  return view;
}

function operationDescription(operation) {
  const [kind, detail] = Object.entries(operation.kind)[0] ?? ["Operation", {}];
  const item = detail?.item ? ` ${detail.item}` : "";
  const timestamp = operation.timestamp;
  return `${kind}${item} · ${timestamp.replica}:${timestamp.counter}`;
}

function renderOutbox(element, operations) {
  element.replaceChildren();
  if (operations.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty";
    empty.textContent = "Nothing waiting.";
    element.append(empty);
    return;
  }

  for (const operation of operations) {
    const item = document.createElement("li");
    item.textContent = operationDescription(operation);
    element.append(item);
  }
}

function renderReplica(element, replica, snapshot) {
  element.replaceChildren();
  snapshot.items.forEach((item, index) => {
    const row = document.createElement("li");
    row.className = "item";

    const copy = document.createElement("div");
    copy.className = "item-copy";
    const title = document.createElement("strong");
    title.textContent = item.value;
    const identity = document.createElement("span");
    identity.textContent = `stable id: ${item.id}`;
    copy.append(title, identity);

    const actions = document.createElement("div");
    actions.className = "item-actions";
    const up = document.createElement("button");
    up.type = "button";
    up.textContent = "↑";
    up.title = `Move ${item.value} up on this replica`;
    up.setAttribute("aria-label", up.title);
    up.disabled = index === 0;
    up.dataset.action = "move";
    up.dataset.replica = replica;
    up.dataset.item = item.id;
    up.dataset.index = String(index - 1);

    const down = document.createElement("button");
    down.type = "button";
    down.textContent = "↓";
    down.title = `Move ${item.value} down on this replica`;
    down.setAttribute("aria-label", down.title);
    down.disabled = index === snapshot.items.length - 1;
    down.dataset.action = "move";
    down.dataset.replica = replica;
    down.dataset.item = item.id;
    down.dataset.index = String(index + 1);

    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Delete";
    remove.className = "delete";
    remove.dataset.action = "delete";
    remove.dataset.replica = replica;
    remove.dataset.item = item.id;

    actions.append(up, down, remove);
    row.append(copy, actions);
    element.append(row);
  });
}

function render() {
  readView();
  renderReplica(leftList, "left", view.left);
  renderReplica(rightList, "right", view.right);
  renderOutbox(leftOutbox, view.leftOutbox);
  renderOutbox(rightOutbox, view.rightOutbox);

  const pending = view.leftOutbox.length + view.rightOutbox.length;
  status.dataset.converged = String(view.converged);
  status.textContent = view.converged
    ? "Replicas contain the same CRDT state."
    : pending > 0
      ? "Replicas are partitioned; deliver the waiting operations to reconcile them."
      : "Visible results differ even though no operations are waiting.";

  document.querySelector('[data-deliver="left"]').disabled = view.leftOutbox.length === 0;
  document.querySelector('[data-deliver="right"]').disabled = view.rightOutbox.length === 0;
  document.querySelector("#sync-button").disabled = pending === 0;
}

function resetLab() {
  lab = new TwoReplicaListLab();
  setError(null);
  render();
}

document.addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (!button) return;

  try {
    if (button.dataset.action === "move") {
      lab.move_item(button.dataset.replica, button.dataset.item, Number(button.dataset.index));
      render();
      return;
    }

    if (button.dataset.action === "delete") {
      lab.delete_item(button.dataset.replica, button.dataset.item);
      render();
      return;
    }

    if (button.dataset.deliver) {
      lab.deliver_next(button.dataset.deliver);
      render();
    }
  } catch (error) {
    setError(error);
  }
});

document.querySelector("#sync-button").addEventListener("click", () => {
  try {
    lab.deliver_all();
    setError(null);
    render();
  } catch (error) {
    setError(error);
  }
});

document.querySelector("#reset-button").addEventListener("click", resetLab);

document.querySelector("#conflict-button").addEventListener("click", () => {
  try {
    lab = new TwoReplicaListLab();
    lab.move_item("left", "charlie", 0);
    lab.move_item("right", "charlie", 3);
    setError(null);
    render();
  } catch (error) {
    setError(error);
  }
});

try {
  await init();
  resetLab();
} catch (error) {
  setError(error);
}
