import type { QuickActionsConfig, QuickAction, LocalizedQuickAction, QuickActionsI18n } from '../types/quickAction';
import { i18nService } from './i18n';

import configData from '../../../public/quick-actions.json';
import i18nData from '../../../public/quick-actions-i18n.json';

class QuickActionService {
  private config: QuickActionsConfig = configData as QuickActionsConfig;
  private i18nDataCache: QuickActionsI18n = i18nData as QuickActionsI18n;
  private listeners = new Set<() => void>();

  /**
   * 获取所有快捷操作（已本地化）
   */
  getLocalizedActions(): LocalizedQuickAction[] {
    const language = i18nService.getLanguage();

    return this.config.actions.map(action => {
      const actionI18n = this.i18nDataCache[language]?.[action.id];

      return {
        ...action,
        label: actionI18n?.label || action.id,
        prompts: action.prompts.map(prompt => {
          const promptI18n = actionI18n?.prompts?.[prompt.id];

          return {
            id: prompt.id,
            label: promptI18n?.label || prompt.id,
            description: promptI18n?.description,
            prompt: promptI18n?.prompt || ''
          };
        })
      };
    });
  }

  /**
   * 获取所有快捷操作（原始数据）
   */
  getActions(): QuickAction[] {
    return this.config.actions;
  }

  /**
   * 根据 ID 获取快捷操作（已本地化）
   */
  getLocalizedActionById(id: string): LocalizedQuickAction | undefined {
    const actions = this.getLocalizedActions();
    return actions.find(action => action.id === id);
  }

  /**
   * 根据 ID 获取快捷操作（原始数据）
   */
  getActionById(id: string): QuickAction | undefined {
    const actions = this.getActions();
    return actions.find(action => action.id === id);
  }

  /**
   * 根据 skillMapping 获取对应的快捷操作（已本地化）
   */
  getLocalizedActionBySkillMapping(skillMapping: string): LocalizedQuickAction | undefined {
    const actions = this.getLocalizedActions();
    return actions.find(action => action.skillMapping === skillMapping);
  }

  /**
   * 根据 skillMapping 获取对应的快捷操作（原始数据）
   */
  getActionBySkillMapping(skillMapping: string): QuickAction | undefined {
    const actions = this.getActions();
    return actions.find(action => action.skillMapping === skillMapping);
  }

  /**
   * 订阅语言变化事件
   */
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /**
   * 通知所有订阅者
   */
  private notifyListeners(): void {
    this.listeners.forEach(listener => listener());
  }

  /**
   * 清除缓存（用于重新加载）
   */
  clearCache(): void {
    this.notifyListeners();
  }

  /**
   * 初始化服务（订阅语言变化）
   */
  initialize(): void {
    i18nService.subscribe(() => {
      this.clearCache();
    });
  }
}

export const quickActionService = new QuickActionService();
