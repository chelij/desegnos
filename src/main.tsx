import React from 'react';
import { createRoot } from 'react-dom/client';
import { installUsageLog } from './usage';
import { installHistorySteps } from './historySteps';
import { installWheelStep } from './wheelStep';
import App from './App';
import './styles.css';
import './brand.css';
import './canvas.css';
import './notice.css';
import './ui.css';

installUsageLog();
installHistorySteps();
installWheelStep();
createRoot(document.getElementById('root')!).render(<React.StrictMode><App /></React.StrictMode>);
