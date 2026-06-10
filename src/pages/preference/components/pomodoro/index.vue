<script setup lang="ts">
import { Divider, InputNumber, SpaceAddon, SpaceCompact, Switch } from 'antdv-next'

import ProListItem from '@/components/pro-list-item/index.vue'
import ProList from '@/components/pro-list/index.vue'
import { usePomodoroStore } from '@/stores/pomodoro'

const pomodoroStore = usePomodoroStore()
</script>

<template>
  <ProList :title="$t('pages.preference.pomodoro.labels.pomodoroSettings')">
    <ProListItem
      :description="$t('pages.preference.pomodoro.hints.enabled')"
      :title="$t('pages.preference.pomodoro.labels.enabled')"
    >
      <Switch v-model:checked="pomodoroStore.enabled" />
    </ProListItem>
  </ProList>

  <ProList
    v-if="pomodoroStore.enabled"
    :title="$t('pages.preference.pomodoro.labels.timerSettings')"
  >
    <ProListItem
      :description="$t('pages.preference.pomodoro.hints.workDuration')"
      :title="$t('pages.preference.pomodoro.labels.workDuration')"
    >
      <SpaceCompact>
        <InputNumber
          v-model:value="pomodoroStore.settings.workDuration"
          class="w-20"
          :min="1"
          :max="60"
        />
        <SpaceAddon>min</SpaceAddon>
      </SpaceCompact>
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.pomodoro.hints.shortBreakDuration')"
      :title="$t('pages.preference.pomodoro.labels.shortBreakDuration')"
    >
      <SpaceCompact>
        <InputNumber
          v-model:value="pomodoroStore.settings.shortBreakDuration"
          class="w-20"
          :min="1"
          :max="30"
        />
        <SpaceAddon>min</SpaceAddon>
      </SpaceCompact>
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.pomodoro.hints.longBreakDuration')"
      :title="$t('pages.preference.pomodoro.labels.longBreakDuration')"
    >
      <SpaceCompact>
        <InputNumber
          v-model:value="pomodoroStore.settings.longBreakDuration"
          class="w-20"
          :min="1"
          :max="60"
        />
        <SpaceAddon>min</SpaceAddon>
      </SpaceCompact>
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.pomodoro.hints.longBreakInterval')"
      :title="$t('pages.preference.pomodoro.labels.longBreakInterval')"
    >
      <InputNumber
        v-model:value="pomodoroStore.settings.longBreakInterval"
        class="w-20"
        :min="2"
        :max="10"
      />
    </ProListItem>

    <Divider />

    <ProListItem
      :description="$t('pages.preference.pomodoro.hints.autoStartBreak')"
      :title="$t('pages.preference.pomodoro.labels.autoStartBreak')"
    >
      <Switch v-model:checked="pomodoroStore.settings.autoStartBreak" />
    </ProListItem>

    <ProListItem
      :description="$t('pages.preference.pomodoro.hints.autoStartWork')"
      :title="$t('pages.preference.pomodoro.labels.autoStartWork')"
    >
      <Switch v-model:checked="pomodoroStore.settings.autoStartWork" />
    </ProListItem>
  </ProList>
</template>
