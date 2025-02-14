<template>
  <Teleport v-if="isOpen" to="body">
    <div ref="internalRef" :style="computedStyle" class="absolute z-50 ml-sm">
      <slot />
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import {
  ref,
  onBeforeUnmount,
  PropType,
  computed,
  onMounted,
  onUnmounted,
} from "vue";
import * as _ from "lodash-es";

const props = defineProps({
  anchorTo: { type: Object },
  // anchorDirectionX determines the direction the Popover pops out from its anchoring element, left or right
  anchorDirectionX: {
    type: String as PropType<"left" | "right">,
    default: "right",
  },
  // anchorPositionY determines how the Popover aligns to its anchor - aligning the top edges, bottom edges, or middles
  anchorAlignY: {
    type: String as PropType<"top" | "middle" | "bottom">,
    default: "middle",
  },
  // override the default positioning logic and give the popover a fixed position
  fixedPosition: { type: Object as PropType<{ x: number; y: number }> },
});

const internalRef = ref<HTMLElement>();
const isOpen = ref(false);
const isRepositioning = ref(false);
const anchorEl = ref<HTMLElement>();
const anchorPos = ref<{ x: number; y: number }>();

function onWindowMousedown(e: MouseEvent) {
  requestAnimationFrame(() => {
    if (e.target instanceof Element && internalRef.value?.contains(e.target)) {
      return; // Don't close on click inside popover
    }

    close();
  });
}

function onKeyboardEvent(e: KeyboardEvent) {
  if (e.key === "Escape") {
    close();
  }
}

function removeListeners() {
  window.removeEventListener("click", onWindowMousedown);
  window.removeEventListener("keydown", onKeyboardEvent);
}

onBeforeUnmount(() => {
  removeListeners();
});

function nextFrame(cb: () => void) {
  requestAnimationFrame(() => requestAnimationFrame(cb));
}

function open(e?: MouseEvent, anchorToMouse?: boolean) {
  const clickTargetIsElement = e?.target instanceof HTMLElement;

  if (props.anchorTo) {
    // can anchor to a specific element via props
    anchorEl.value = props.anchorTo.$el;
  } else if (e && (anchorToMouse || !clickTargetIsElement)) {
    // or can anchor to mouse position if anchorToMouse is true (or event has not target)
    anchorEl.value = undefined;
    anchorPos.value = { x: e?.clientX, y: e.clientY };
  } else if (clickTargetIsElement) {
    // otherwise anchor to click event target
    anchorEl.value = e.target;
  } else {
    // shouldn't happen...?
    anchorEl.value = undefined;
  }

  isRepositioning.value = true;
  isOpen.value = true;

  nextFrame(finishOpening);
}

function finishOpening() {
  startListening();
  readjustPosition();
}

function startListening() {
  window.addEventListener("keydown", onKeyboardEvent);
  window.addEventListener("mousedown", onWindowMousedown);
}

function readjustPosition() {
  if (!internalRef.value) return;
  isRepositioning.value = false;

  if (props.fixedPosition) {
    anchorPos.value = { x: props.fixedPosition.x, y: props.fixedPosition.y };
    return;
  }

  let anchorRect;
  if (anchorEl.value) {
    anchorRect = anchorEl.value.getBoundingClientRect();
  } else if (anchorPos.value) {
    anchorRect = new DOMRect(anchorPos.value.x, anchorPos.value.y);
  } else {
    throw new Error("Menu must be anchored to an element or mouse position");
  }
  const popoverRect = internalRef.value.getBoundingClientRect();
  anchorPos.value = { x: 0, y: 0 };

  const windowWidth = document.documentElement.clientWidth;
  if (props.anchorDirectionX === "left") {
    anchorPos.value.x = windowWidth - anchorRect.left;
  } else {
    anchorPos.value.x = anchorRect.right;
  }

  if (props.anchorAlignY === "bottom") {
    anchorPos.value.y = anchorRect.bottom - popoverRect.height;
  } else if (props.anchorAlignY === "top") {
    anchorPos.value.y = anchorRect.top;
  } else {
    anchorPos.value.y =
      anchorRect.top + anchorRect.height / 2 - popoverRect.height / 2;
  }
}

const computedStyle = computed(() => {
  if (anchorPos.value) {
    const style: Record<string, string> = {};

    if (props.anchorDirectionX === "left") {
      style.right = `${anchorPos.value.x}px`;
    } else {
      style.left = `${anchorPos.value.x}px`;
    }
    style.top = `${anchorPos.value.y}px`;

    return style;
  } else {
    return { display: "hidden" };
  }
});

function close() {
  isOpen.value = false;
  anchorPos.value = undefined;
  removeListeners();
}

// If the browser window is resized, close the Popover
// TODO(Wendy) - Close the Popover if the element it is anchored to is scrolled away from its starting position by more than a certain amount
const closeOnResize = _.debounce(close, 1000, {
  leading: true,
  trailing: false,
});

onMounted(() => {
  window.addEventListener("resize", closeOnResize);
});

onUnmounted(() => {
  window.removeEventListener("resize", closeOnResize);
});

defineExpose({ open, close, isOpen });
</script>
