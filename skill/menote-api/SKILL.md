---
name: menote-api
description: 通过 RESTful API 管理 MeNote 知识库。触发条件：对话必须同时包含 "menote"（或 "MeNote"）和明确的操作意图（如"查笔记"、"写笔记"、"删笔记"、"改笔记"、"看分类"、"加分类"、"改分类"、"删分类"、"上传附件"等）。仅提到 menote 但无操作意图时不触发。
---

# MeNote Knowledge Base API Skill

## Description

This skill provides the ability to manage notes and categories on a MeNote knowledge base system via RESTful API. You can list, create, edit, delete, and pin notes, as well as manage categories using token-based authentication.

## When to Use

Use this skill when the user wants to:
- Query, list, or search notes
- View note details
- Create new notes
- Edit existing notes
- Delete notes
- Pin/unpin notes
- Query or list categories (tree structure)
- Create, edit, sort, or delete categories
- Upload attachments (images, documents)

## Configuration

Before making any API call, read the config file to get the token and base URL:

- **Config file**: `config.json`（与 SKILL.md 同目录）
- **Contents**:
  ```json
  {
    "base_url": "http://127.0.0.1:3107",
    "token": "mk_xxx..."
  }
  ```

Always read this file at the start of each menote-api task to obtain the current `base_url` and `token`. Do NOT hardcode these values.

## HTTP Headers

Every API request **must** include this header to get a JSON response (without it the server returns an HTML page):

```
X-Requested-With: XMLHttpRequest
```

## Authentication

All API requests require a valid token. The token is stored in the config file above. It can be passed in two ways:

1. **Query parameter**: `?token=YOUR_TOKEN_STRING`
2. **Authorization header**: `Authorization: Bearer YOUR_TOKEN_STRING`

The token is generated from the admin panel (管理后台 → Token 管理). Each token has specific permissions (read/create/edit/delete for categories and notes).

**Important**: Token access is restricted to `cate` and `note` controllers only. Other endpoints (e.g., `p2p`, `user`) require cookie-based admin login.

## API Base URL

The base URL is stored in the config file. All API endpoints are relative to it. For example, if `base_url` is `http://127.0.0.1:3107`, the full URL for listing notes would be `http://127.0.0.1:3107/api/note/list`.

## API Endpoints

### Response Format

All responses follow this format:
- Success: `{"state": 1, "msg": "...", "data": ...}`
- Error: `{"state": 0, "msg": "...", "data": null}`

---

### Category APIs

#### Get Category Tree

Get all categories in tree structure (supports nested categories).

- **URL**: `/api/cate/tree`
- **Method**: GET
- **Parameters**: `token` (required)
- **Response**:
```json
{
  "state": 1,
  "msg": "success",
  "data": [
    {
      "id": 1,
      "name": "工作项目",
      "icon": "💼",
      "parent_id": 0,
      "children": [
        {
          "id": 5,
          "name": "三级分类",
          "icon": "🍎",
          "parent_id": 1,
          "children": []
        }
      ]
    },
    {
      "id": 2,
      "name": "学习笔记",
      "icon": "📚",
      "parent_id": 0,
      "children": []
    }
  ]
}
```

#### List Categories

Get flat list of all categories.

- **URL**: `/api/cate/list`
- **Method**: GET
- **Parameters**: `token` (required)
- **Response**: Same structure as tree but flattened.

#### Create Category

Create a new category.

- **URL**: `/api/cate/create`
- **Method**: POST
- **Parameters**: `token` (required), plus body:
  - `name` (required): Category name
  - `icon`: Category icon emoji (optional, default: "📁")
  - `parent_id`: Parent category ID (optional, default: 0 for root level)
  - `sort`: Sort order (optional, default: auto-increment)
- **Response**:
```json
{
  "state": 1,
  "msg": "新增成功",
  "data": { "id": 6 }
}
```

#### Edit Category

Update an existing category.

- **URL**: `/api/cate/edit`
- **Method**: POST
- **Parameters**: `token` (required), plus body:
  - `id` (required): Category ID
  - `name`: Category name (optional)
  - `icon`: Icon emoji (optional)
  - `parent_id`: Parent ID (optional)
  - `sort`: Sort order (optional)
- **Response**:
```json
{
  "state": 1,
  "msg": "保存成功",
  "data": null
}
```

#### Sort Categories

Drag-and-drop sort (reorder categories).

- **URL**: `/api/cate/sort`
- **Method**: POST
- **Parameters**: `token` (required), plus body:
  - `ids` (required): Array of category IDs in new order, e.g., `[1, 2, 5]`
- **Response**:
```json
{
  "state": 1,
  "msg": "排序成功",
  "data": null
}
```

#### Delete Category

Delete a category (and optionally its child categories).

- **URL**: `/api/cate/delete`
- **Method**: POST
- **Parameters**: `token` (required), `id` (required)
- **Response**:
```json
{
  "state": 1,
  "msg": "删除成功",
  "data": null
}
```

---

### Note APIs

#### List Notes

Get notes with pagination and filtering.

- **URL**: `/api/note/list`
- **Method**: GET
- **Parameters**:
  - `token` (required)
  - `page`: Page number (default: 1)
  - `rows`: Items per page (default: 20, max: 100)
  - `cate_id`: Filter by category ID (optional, 0 = all)
  - `keyword`: Search keyword (optional, searches title and content)
  - `q`: Search keyword (optional, searches title only)
  - `order`: Sort order (optional). `latest` = by update_time desc (pinned still first); omit for custom sort (is_pinned desc, sort asc, add_time desc)
  - `is_pinned`: Filter pinned notes: 1=pinned only, 0=non-pinned only (optional)
  - **Note**: list responses do NOT include `content` (only `excerpt`, max 110 chars). Use the detail API to fetch full content.
- **Response**:
```json
{
  "state": 1,
  "msg": "success",
  "data": {
    "list": [
      {
        "id": 28,
        "title": "图片上传测试",
        "cate_id": 4,
        "cate_name": "生活随笔",
        "keywords": "",
        "is_pinned": 0,
        "excerpt": "First 110 chars of the note body with markdown syntax stripped…",
        "attach_count": 2,
        "attach_size": 682240,
        "add_time": 1726012800,
        "update_time": 1726012800
      }
    ],
    "page": 1,
    "rows": 20,
    "total": 32
  }
}
```

#### Get Note Detail

Get full details of a specific note including content.

- **URL**: `/api/note/detail`
- **Method**: GET
- **Parameters**: `token` (required), `id` (required)
- **Response**:
```json
{
  "state": 1,
  "msg": "success",
  "data": {
    "id": 28,
    "title": "图片上传测试",
    "cate_id": 4,
    "cate_name": "生活随笔",
    "content": "# Note content in Markdown\n\n...",
    "keywords": "",
    "is_pinned": 0,
    "add_time": 1726012800,
    "update_time": 1726012800
  }
}
```

#### Create Note

Create a new note. Content supports Markdown format.

- **URL**: `/api/note/create`
- **Method**: POST
- **Parameters**: `token` (required), plus body:
  - `title` (required): Note title
  - `cate_id` (required): Category ID
  - `content`: Note content in Markdown (optional, default: "")
  - `keywords`: Keywords/tags, comma-separated (optional)
  - `is_pinned`: Pin status: 0=normal, 1=pinned (optional, default: 0)
- **Response**:
```json
{
  "state": 1,
  "msg": "新增成功",
  "data": { "id": 33 }
}
```

#### Edit Note

Update an existing note. Uses optimistic locking via `update_time`.

- **URL**: `/api/note/edit`
- **Method**: POST
- **Parameters**: `token` (required), plus body:
  - `id` (required): Note ID
  - `title`: Note title (optional)
  - `cate_id`: Category ID (optional)
  - `content`: Note content (optional)
  - `keywords`: Keywords (optional)
  - `is_pinned`: Pin status (optional)
  - `update_time` (required for content edits): Current update_time from detail response (optimistic lock)
- **Response** (success):
```json
{
  "state": 1,
  "msg": "保存成功",
  "data": { "update_time": 1726013000 }
}
```
**Important**: On success, `data.update_time` contains the new version. Update your local copy.

**Response** (conflict — another edit happened concurrently):
```json
{
  "state": 0,
  "msg": "保存失败",
  "data": { "conflict": true }
}
```
When conflict occurs, fetch the latest note via `/api/note/detail` and retry.

#### Pin/Unpin Note

Toggle pin status of a note.

- **URL**: `/api/note/pin`
- **Method**: POST
- **Parameters**: `token` (required), `id` (required)
- **Response**:
```json
{
  "state": 1,
  "msg": "操作成功",
  "data": null
}
```
Note: This does NOT update `update_time` (to avoid false conflicts with concurrent edits).

#### Sort Notes

Reorder notes within a category.

- **URL**: `/api/note/sort`
- **Method**: POST
- **Parameters**: `token` (required), plus body:
  - `ids` (required): Array of note IDs in new order
- **Response**:
```json
{
  "state": 1,
  "msg": "排序成功",
  "data": null
}
```
Note: This does NOT update `update_time`.

#### Delete Note

Delete a note.

- **URL**: `/api/note/delete`
- **Method**: POST
- **Parameters**: `token` (required), `id` (required)
- **Response**:
```json
{
  "state": 1,
  "msg": "删除成功",
  "data": null
}
```

---

### Attachment APIs

#### Upload File

Upload an image or document attachment. Associates with a note if `note_id` is provided.

- **URL**: `/api/upload/index`
- **Method**: POST (multipart/form-data)
- **Parameters**: `token` (required), plus form fields:
  - `file` (required): The file to upload
  - `note_id`: Note ID to associate (optional, default: 0)
- **Headers**: Must include `X-Requested-With: XMLHttpRequest`
- **Response**:
```json
{
  "state": 1,
  "msg": "上传成功",
  "data": {
    "id": 14,
    "url": "/upload/2026/0918/abc123.png",
    "filename": "screenshot.png",
    "filesize": 12345
  }
}
```

Max file size: 10 MB. Supported formats: images (.png, .jpg, .gif, etc.), documents (.pdf, .doc, .docx, .xls, .xlsx, .ppt, .pptx, .txt), archives (.zip, .rar, .7z).

---

## Permission System

Tokens have granular permissions using a bitmask system:

| Permission | Bit | Value | Description |
|-----------|-----|-------|-------------|
| Category Read | 0 | 1 | List/view categories |
| Category Create | 1 | 2 | Create categories |
| Category Edit | 2 | 4 | Edit/sort categories |
| Category Delete | 3 | 8 | Delete categories |
| Note Read | 4 | 16 | List/view notes |
| Note Create | 5 | 32 | Create notes |
| Note Edit | 6 | 64 | Edit/pin/sort notes |
| Note Delete | 7 | 128 | Delete notes |

Permission values can be combined. For example, `255` (all bits set) grants full access.

## Usage Examples

### Example 1: List all notes

```bash
curl -H "X-Requested-With: XMLHttpRequest" \
  "http://127.0.0.1:3107/api/note/list?token=mk_xxx"
```

### Example 2: Create a new note

```bash
curl -X POST "http://127.0.0.1:3107/api/note/create?token=mk_xxx" \
  -H "Content-Type: application/json" \
  -H "X-Requested-With: XMLHttpRequest" \
  -d '{
    "title": "My New Note",
    "cate_id": 1,
    "content": "# Hello World\n\nThis is my new note.",
    "keywords": "hello,world"
  }'
```

### Example 3: Get category tree

```bash
curl -H "Authorization: Bearer mk_xxx" \
  -H "X-Requested-With: XMLHttpRequest" \
  "http://127.0.0.1:3107/api/cate/tree"
```

### Example 4: Upload an image with note association

```bash
curl -X POST "http://127.0.0.1:3107/api/upload/index?token=mk_xxx" \
  -H "X-Requested-With: XMLHttpRequest" \
  -F "file=@screenshot.png" \
  -F "note_id=28"
```

## Notes

- Note content supports **Markdown** format (via Vditor editor)
- `add_time` and `update_time` are Unix timestamps (seconds)
- **Optimistic locking**: When editing note content, you MUST pass the current `update_time`. If it doesn't match the database, the edit is rejected with `data.conflict: true`. Fetch the latest version and retry.
- Metadata operations (pin, sort) do NOT update `update_time` to avoid false conflicts.
- Token access is limited to `cate` and `note` controllers. P2P management and user settings require cookie-based admin login.
- Uploaded files are stored in `public/upload/YYYY/MMDD/` with MD5-hashed filenames.

## ⚠️ PowerShell Submission Notes

**Do NOT use PowerShell to submit notes containing Markdown backticks (```) or complex Unicode content**. Reasons:

1. Backtick `` ` `` is an escape character in PowerShell, causing content corruption
2. `ConvertTo-Json` handles nested strings and special characters unreliably
3. `Invoke-RestMethod` may lose non-ASCII characters during encoding conversion

**Solution**: Use Python `urllib` + `json.dumps` for submission.

### Python Submission Example

```python
import urllib.request, json

# Read config from config.json
with open("config.json") as f:
    cfg = json.load(f)

content = r"""Your Markdown content here..."""

data = json.dumps({
    "title": "Note Title",
    "cate_id": 1,
    "content": content,
    "keywords": "tag1,tag2"
}).encode("utf-8")

req = urllib.request.Request(
    f"{cfg['base_url']}/api/note/create",
    data=data,
    method="POST",
    headers={
        "Content-Type": "application/json; charset=utf-8",
        "X-Requested-With": "XMLHttpRequest",
        "Authorization": f"Bearer {cfg['token']}"
    }
)

with urllib.request.urlopen(req) as resp:
    result = json.loads(resp.read().decode("utf-8"))
    print(json.dumps(result, ensure_ascii=False, indent=2))
```

For editing notes, change the URL to `/api/note/edit` and add `"id": NOTE_ID` and `"update_time": CURRENT_UPDATE_TIME` to the body.
