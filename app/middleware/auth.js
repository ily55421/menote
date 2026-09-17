const {Middleware} = require('jj.js');

class Auth extends Middleware
{
    async notefile() {
        if(await this.$model.user.is_login()) return;
        const notefile = this.$request.param('notefile');this.$logger.info(notefile);
        if(!notefile) return;
        const filepath = '/upload/' + notefile.split('?')[0];this.$logger.info(filepath);
        const note_id = await this.$db.table('attach').where({filepath}).withCache(600).value('note_id');this.$logger.info(note_id);
        if(!note_id) return;
        const cate_id = await this.$db.table('note').where({id: note_id}).value('cate_id');this.$logger.info(cate_id);
        if(!cate_id) return;
        const is_public = await this.$db.table('cate').where({id: cate_id}).value('is_public');this.$logger.info(is_public);
        if(is_public == 1) return;
        this.$logger.warning('Unauthorized access to private note file: ' + notefile);
        this.ctx.status = 403;
    }
}

module.exports = Auth;
