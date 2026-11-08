import { useCallback, useLayoutEffect, useRef, useState } from 'react';
import type { WorkspaceView } from '../types';

type NoticeItem = { id: number; message: string; undo?: () => void; history: string };

const historyKeyOf = (workspace: WorkspaceView | null) => workspace?.project ? `${workspace.project.id}:${workspace.project.revision}` : '';

export function useNotices(current: { current: WorkspaceView | null }, view: WorkspaceView | null) {
  const [notices, setNotices] = useState<NoticeItem[]>([]);
  const noticeId = useRef(0);
  const setNotice = useCallback((message: string, undo?: () => void) => {
    const id = ++noticeId.current;
    setNotices(items => [...items.filter(item => item.message !== message), { id, message, undo, history: historyKeyOf(current.current) }]);
  }, []);
  const dismissNotice = useCallback((id: number) => setNotices(items => items.filter(item => item.id !== id)), []);
  const historyKey = historyKeyOf(view);
  useLayoutEffect(() => {
    setNotices(items => {
      const kept = items.filter(item => !item.undo || item.history === historyKey);
      return kept.length === items.length ? items : kept;
    });
  }, [historyKey]);
  return { notices, setNotice, dismissNotice };
}
