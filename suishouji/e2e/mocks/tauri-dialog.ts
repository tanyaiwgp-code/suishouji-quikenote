// E2E mock：系统对话框（避免真弹系统窗口）。open/save 均返回 null = 用户取消。
export const open = async (): Promise<null> => null;
export const save = async (): Promise<null> => null;
