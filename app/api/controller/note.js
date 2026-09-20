const Base = require('./base');

class Note extends Base
{
    async list() {
        const page = parseInt(this.$request.get('page', 1)) || 1;
        // 上限 100：列表要为每行跑摘要正则，且 getAttachStats 会展开等长的 IN 占位符
        const rows = Math.min(Math.max(parseInt(this.$request.get('rows', 20)) || 20, 1), 100);
        const cateId = parseInt(this.$request.get('cate_id', 0)) || 0;
        const keyword = this.$request.get('keyword', '');
        const q = this.$request.get('q', '');
        // order=latest 供「最新」入口使用：按更新时间倒序而非自定义排序
        const order = this.$request.get('order', '');

        const condition = {};
        if(cateId > 0) condition['n.cate_id'] = cateId;
        if(keyword) condition['n.keywords'] = ['like', '%' + keyword + '%'];
        if(q) condition['n.title'] = ['like', '%' + q + '%'];

        const [list, pagination] = await this.$model.note.getNoteList(condition, rows, page, order);

        // 列表卡片需要摘要与附件信息；列表只取必要字段，正文不下发（避免大响应）
        const attachStats = await this.$model.note.getAttachStats(list.map(item => item.id));
        for(const item of list) {
            item.excerpt = this._excerpt(item.content);
            delete item.content;
            const stat = attachStats[item.id];
            item.attach_count = stat ? stat.count : 0;
            item.attach_size = stat ? stat.size : 0;
        }

        this.$success('success', {list, page, rows, total: pagination.total()});
    }

    /**
     * 从 Markdown 正文提取纯文本摘要（去代码块/图片/链接语法，压缩空白）
     * @private
     */
    _excerpt(content, length = 110) {
        if(!content) return '';
        let text = String(content)
            .replace(/```[\s\S]*?```/g, ' ')       // 代码块
            .replace(/`[^`]*`/g, ' ')              // 行内代码
            .replace(/!\[[^\]]*\]\([^)]*\)/g, ' ') // 图片
            .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1') // 链接保留文字
            .replace(/^\s{0,3}#{1,6}\s+/gm, '')    // 标题井号
            .replace(/^\s{0,3}>\s?/gm, '')         // 引用
            .replace(/^\s{0,3}[-*+]\s+/gm, '')     // 无序列表
            .replace(/^\s{0,3}\d+\.\s+/gm, '')     // 有序列表
            .replace(/[*_~]/g, '')                 // 强调符
            .replace(/\|/g, ' ')                   // 表格分隔
            .replace(/\s+/g, ' ')
            .trim();
        return text.length > length ? text.slice(0, length) + '…' : text;
    }

    async detail() {
        const id = this.$request.get('id', 0);
        if(!id) return this.$error('缺少id参数');

        const note = await this.$db.table('note n')
            .field('n.*, c.name as cate_name')
            .join('cate c', 'n.cate_id=c.id', 'left')
            .where({'n.id': id})
            .find();
        
        if(!note) return this.$error('笔记不存在');

        // 获取反向链接
        const backlinks = await this.$model.note.getBacklinks(id);
        note.backlinks = backlinks;

        this.$success('success', note);
    }

    async create() {
        if(!this.$request.isPost()) return this.$error('请使用POST请求');

        const data = this.$request.postAll();
        if(!data.title) return this.$error('标题不能为空');

        const id = await this.$model.note.saveNote(data);
        if(id) {
            this.$success('创建成功', {id});
        } else {
            this.$error('创建失败');
        }
    }

    async edit() {
        if(!this.$request.isPost()) return this.$error('请使用POST请求');

        const data = this.$request.postAll();
        if(!data.id) return this.$error('缺少id参数');

        const note = await this.$db.table('note').where({id: data.id}).find();
        if(!note) return this.$error('笔记不存在');

        // 乐观锁：请求携带客户端加载数据时的 update_time，与数据库当前值不一致，
        // 说明其他地方（另一窗口/设备/外部 API）已编辑保存过，拒绝本次保存防止覆盖。
        // 未携带 update_time 的调用（旧客户端、外部 API 局部更新）不校验，保持兼容。
        if(data.update_time !== undefined && data.update_time !== null
            && Number(note.update_time) !== Number(data.update_time)) {
            return this.$error('保存失败：该笔记已在其他地方被修改，请先备份本地内容，重新获取数据后再编辑保存', {conflict: true, update_time: note.update_time});
        }

        const result = await this.$model.note.saveNote(data);
        if(result) {
            // 返回新的 update_time，前端刷新本地乐观锁基准（否则连续保存会自我冲突）
            this.$success('保存成功', {update_time: result});
        } else {
            this.$error('保存失败');
        }
    }

    async delete() {
        const id = this.$request.get('id', 0);
        if(!id) return this.$error('缺少id参数');

        try {
            await this.$db.startTrans(async () => {
                await this.$db.table('note').delete({id});
                await this.$db.table('note_link').delete({source_id: id});
                await this.$db.table('note_link').delete({target_id: id});
            });
            this.$success('删除成功');
        } catch(e) {
            this.$error('删除失败：' + e.message);
        }
    }

    async sort() {
        if(!this.$request.isPost()) return this.$error('请使用POST请求');

        const items = this.$request.post('items', []);
        if(!Array.isArray(items) || items.length === 0) {
            return this.$error('参数错误');
        }

        try {
            for(const item of items) {
                await this.$db.table('note').where({id: item.id}).update({sort: item.sort});
            }
            this.$success('排序已保存');
        } catch(e) {
            this.$error('保存失败：' + e.message);
        }
    }

    async backlinks() {
        const id = this.$request.get('id', 0);
        if(!id) return this.$error('缺少id参数');

        const backlinks = await this.$model.note.getBacklinks(id);
        this.$success('success', backlinks);
    }

    async pin() {
        if(!this.$request.isPost()) return this.$error('请使用POST请求');

        const id = this.$request.post('id', 0);
        const isPinned = this.$request.post('is_pinned', 0);

        if(!id) return this.$error('缺少id参数');

        try {
            // 仅改置顶标记，不更新 update_time：避免纯置顶操作让其他端
            // 正在编辑的笔记产生虚假的乐观锁冲突
            await this.$db.table('note').where({id}).update({
                is_pinned: isPinned ? 1 : 0
            });
            this.$success(isPinned ? '已置顶' : '已取消置顶');
        } catch(e) {
            this.$error('操作失败：' + e.message);
        }
    }
}

module.exports = Note;
