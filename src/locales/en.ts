export default {
  mode: {
    title: 'Mode Settings',
    select: 'Select Mode',
    standard: 'Standard Mode',
    keyboard: 'Keyboard Mode',
  },
  window: {
    title: 'Window Settings',
    penetrable: {
      title: 'Window Penetration',
      description: 'When enabled, the window will not affect operations on other applications',
    },
    size: {
      title: 'Window Size',
      description: 'After moving the mouse to the edge of the window, you can also drag to adjust the window size',
      default: 'Default',
    },
    opacity: {
      title: 'Opacity',
    },
    mirror: {
      title: 'Mirror Mode',
    },
  },
  menu: {
    preference: 'Preferences...',
    hideCat: 'Hide Cat',
    showCat: 'Show Cat',
    catMode: 'Cat Mode',
    windowPenetrable: 'Window Penetration',
    windowSize: 'Window Size',
    opacity: 'Opacity',
    mirrorMode: 'Mirror Mode',
  },
  preference: {
    catSettings: 'Cat Settings',
    generalSettings: 'General Settings',
    modelManagement: 'Model Management',
    about: 'About',
  },
  general: {
    appSettings: 'Application Settings',
    autostart: 'Start at Login',
    updateSettings: 'Update Settings',
    autoCheckUpdate: 'Auto Check for Updates',
  },
  permissions: {
    title: 'Permission Settings',
    inputMonitoring: {
      title: 'Input Monitoring Permission',
      description: 'Enable input monitoring permission to receive system keyboard and mouse events to respond to your operations.',
      authorized: 'Authorized',
      authorize: 'Authorize',
      message: 'If the permission is already enabled, select it and click the "-" button to delete it, then manually add it again, and restart the application to ensure the permission takes effect.',
      messageTitle: 'Input Monitoring Permission',
      messageOk: 'Go to Enable',
    },
  },
  about: {
    title: 'About',
    version: 'Version: v{version}',
    checkUpdate: 'Check for Updates',
    openSource: 'Open Source',
    feedback: 'Feedback',
  },
  main: {
    redrawing: 'Redrawing...',
  },
  model: {
    comingSoon: 'Coming Soon',
  },
  common: {
    percent: '{value}%',
  },
  tray: {
    restart: 'Restart App',
    exit: 'Exit App',
  },
  update: {
    checking: 'Checking for updates...',
    latest: 'Already the latest version🎉',
    newVersion: 'New version found🥳',
    version: 'Update version',
    time: 'Update time',
    changelog: 'Changelog',
    later: 'Update later',
    now: 'Update now',
  },
}
