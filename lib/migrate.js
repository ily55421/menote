/**
 * 数据库索引补齐（幂等迁移）
 *
 * 背景：索引定义在 app/install/menote_sqlite.sql 中，但那只在「首次安装」时执行。
 * 已安装的数据库不会重新跑建表脚本，因此后续新增的索引需要在此补齐。
 *
 * 设计要点：
 *   - CREATE INDEX IF NOT EXISTS：重复执行无副作用，可安全每次启动运行
 *   - 直接用 sqlite3 驱动：jj.js 的 Db 依赖请求上下文（内部要读 async_hooks store），
 *     启动阶段没有上下文会抛 "Cannot destructure property '$' of 'store'"，
 *     故此处绕开框架、直接连库
 *   - 全程 try/catch：索引创建失败只记警告，绝不阻断服务启动
 *   - 与 app/install/menote_sqlite.sql 的定义保持一致（新增索引时两处同步）
 */
const fs = require('fs');
const path = require('path');
const sqlite3 = require('sqlite3');
const {Logger} = require('jj.js');

// 索引清单：[名称, 表, 列定义]
// 覆盖 getNoteList 的两种排序路径（自定义排序 / 「最新」入口）与附件统计
const INDEXES = [
    ['idx_note_cate_id', 'menote_note', '`cate_id`'],
    ['idx_note_add_time', 'menote_note', '`add_time`'],
    // 列表默认排序：ORDER BY is_pinned DESC, sort ASC, add_time DESC
    ['idx_note_cate_sort', 'menote_note', '`cate_id`, `is_pinned` DESC, `sort` ASC, `add_time` DESC'],
    // 「最新」入口排序：ORDER BY is_pinned DESC, update_time DESC
    // 注意不能带 cate_id 前缀——该视图跨分类，前缀列反而让索引失效
    ['idx_note_pinned_update', 'menote_note', '`is_pinned` DESC, `update_time` DESC'],
    // 分类内 + 置顶 + 更新时间（分类视图下按最新排序时用）
    ['idx_note_cate_pinned_update', 'menote_note', '`cate_id`, `is_pinned` DESC, `update_time` DESC'],
    ['idx_note_update_time', 'menote_note', '`update_time` DESC'],
    ['idx_cate_pid', 'menote_cate', '`pid`'],
    ['idx_link_source', 'menote_note_link', '`source_id`'],
    ['idx_link_target', 'menote_note_link', '`target_id`'],
    ['idx_attach_note_id', 'menote_attach', '`note_id`'],
    ['idx_token', 'menote_token', '`token`'],
];

function exec(db, sql) {
    return new Promise((resolve, reject) => {
        db.exec(sql, (err) => (err ? reject(err) : resolve()));
    });
}

/**
 * 补齐缺失索引。任何失败都只记录日志，不影响服务启动。
 * @param {string} baseDir 应用根目录（用于定位 data/menote.db）
 */
async function ensureIndexes(baseDir) {
    const dbFile = path.join(baseDir, 'data', 'menote.db');
    // 数据库文件不存在说明尚未安装，跳过（安装流程会建全量索引）
    if(!fs.existsSync(dbFile)) {
        Logger.system('[migrate] 数据库尚未创建，跳过索引检查');
        return;
    }

    const db = new sqlite3.Database(dbFile, sqlite3.OPEN_READWRITE);
    try {
        let ok = 0;
        for(const [name, table, cols] of INDEXES) {
            try {
                await exec(db, `CREATE INDEX IF NOT EXISTS \`${name}\` ON \`${table}\` (${cols})`);
                ok++;
            } catch(e) {
                Logger.warning(`[migrate] 索引 ${name} 创建失败: ${e.message}`);
            }
        }
        Logger.system(`[migrate] 索引检查完成（${ok}/${INDEXES.length}）`);
    } catch(e) {
        Logger.warning('[migrate] 索引补齐跳过: ' + e.message);
    } finally {
        await new Promise(resolve => db.close(() => resolve()));
    }
}

module.exports = {ensureIndexes};
