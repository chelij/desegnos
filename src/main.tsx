import React from 'react';
import { createRoot } from 'react-dom/client';
import { installUsageLog } from './usage';
import App from './App';
import './styles.css';
import './brand.css';
import './notice.css';
import './ui.css';

installUsageLog();
createRoot(document.getElementById('root')!).render(<React.StrictMode><App /></React.StrictMode>);
