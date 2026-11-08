import { useCallback, useEffect, useState } from 'react';
import { native } from '../api';
import { errorMessage } from '../editorShared';
import type { AppSettingsView, ProjectLocationView, RecentProject, RecentProjectsView, WorkspaceView } from '../types';

/** What the start page and Preferences show: recent projects, the launch preference and the default project folder. */
export function useStartPageData({ queue, setError }: {
  queue: <T>(operation: (workspace: WorkspaceView | null) => Promise<T>, reportErrors?: boolean) => Promise<T>;
  setError: (message: string) => void;
}) {
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [appSettings, setAppSettings] = useState<AppSettingsView>({ showStartPage: true });
  const [projectLocation, setProjectLocation] = useState<ProjectLocationView | null>(null);

  const refreshRecent = useCallback(async () => {
    try {
      const value = await native<RecentProjectsView>('get_recent_projects');
      setRecentProjects(value.projects);
      if (value.error) setError(`Recent projects: ${value.error}`);
    } catch (error) { setError(errorMessage(error)); }
  }, []);
  useEffect(() => { void refreshRecent(); }, [refreshRecent]);
  useEffect(() => { void native<ProjectLocationView>('get_project_location').then(setProjectLocation).catch(error => setError(errorMessage(error))); }, []);

  async function forgetRecent(path: string) {
    await queue(async () => { setRecentProjects(await native<RecentProject[]>('forget_recent_project', { path })); }, false);
  }
  async function pinRecent(path: string, pinned: boolean) {
    await queue(async () => { setRecentProjects(await native<RecentProject[]>('set_recent_project_pin', { path, pinned })); }, false);
  }
  async function changeShowStartPage(value: boolean) {
    try { setAppSettings(await native<AppSettingsView>('set_show_start_page', { value })); }
    catch (error) { setError(errorMessage(error)); }
  }
  async function changeProjectLocation(reset: boolean) {
    const path = reset ? null : await native<string | null>('pick_project_location');
    if (!reset && !path) return;
    setProjectLocation(await native<ProjectLocationView>('set_project_location', { path }));
  }
  return { recentProjects, refreshRecent, forgetRecent, pinRecent, appSettings, setAppSettings, changeShowStartPage, projectLocation, changeProjectLocation };
}
