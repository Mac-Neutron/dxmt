#include "dxmt_dynamic.hpp"
#include "dxmt_texture.hpp"

namespace dxmt {
DynamicBuffer::DynamicBuffer(Buffer *buffer, Flags<BufferAllocationFlag> flags) :
    buffer(buffer),
    flags_(flags),
    name_(buffer->current()) {}

void
DynamicBuffer::incRef() {
  refcount_.fetch_add(1u, std::memory_order_acquire);
};

void
DynamicBuffer::decRef() {
  if (refcount_.fetch_sub(1u, std::memory_order_release) == 1u)
    delete this;
};

Rc<BufferAllocation>
DynamicBuffer::allocate(uint64_t coherent_seq_id) {
  std::lock_guard<dxmt::mutex> lock(mutex_);
  Rc<BufferAllocation> ret;
  for (;;) {
    if (fifo.empty()) {
      break;
    }
    auto entry = fifo.front();
    if (entry.will_free_at > coherent_seq_id) {
      break;
    }
    ret = std::move(entry.allocation);
    fifo.pop();
    break;
  }
  if (!ret.ptr())
    ret = buffer->allocate(flags_);
  return ret;
}

void
DynamicBuffer::updateImmediateName(uint64_t current_seq_id, Rc<BufferAllocation> &&allocation, uint32_t suballocation, bool owned_by_command_list) {
  std::lock_guard<dxmt::mutex> lock(mutex_);
  if (!owned_by_command_list_)
    fifo.push(QueueEntry{.allocation = std::move(name_), .will_free_at = current_seq_id});
  name_ = std::move(allocation);
  name_suballocation_ = suballocation;
  owned_by_command_list_ = owned_by_command_list;
  name_piece_ = 0;
}

void
DynamicBuffer::recycle(uint64_t current_seq_id, Rc<BufferAllocation> &&allocation) {
  std::lock_guard<dxmt::mutex> lock(mutex_);
  if (owned_by_command_list_) {
    if (name_.ptr() == allocation.ptr()) {
      owned_by_command_list_ = false;
      auto _ = std::move(allocation);
      return;
    }
  }
  fifo.push(QueueEntry{.allocation = std::move(allocation), .will_free_at = current_seq_id});
}

std::pair<uint64_t, Rc<BufferAllocation>>
DynamicBuffer::lend(uint64_t coherent_seq_id) {
  Rc<BufferAllocation> ret = allocate(coherent_seq_id);
  std::lock_guard<dxmt::mutex> lock(mutex_);
  uint64_t id = next_piece_++;
  lent_[id] = ret;
  return {id, ret};
}

Rc<BufferAllocation>
DynamicBuffer::takeLent(uint64_t id) {
  std::lock_guard<dxmt::mutex> lock(mutex_);
  auto it = lent_.find(id);
  if (it == lent_.end())
    return {};
  Rc<BufferAllocation> ret = std::move(it->second);
  lent_.erase(it);
  return ret;
}

uint64_t
DynamicBuffer::immediatePiece() {
  std::lock_guard<dxmt::mutex> lock(mutex_);
  if (!name_piece_)
    name_piece_ = next_piece_++;
  return name_piece_;
}

void
DynamicBuffer::setImmediatePiece(uint64_t id) {
  std::lock_guard<dxmt::mutex> lock(mutex_);
  name_piece_ = id;
}

void
DynamicBuffer::useImmediateSuballocation(uint32_t suballocation) {
  std::lock_guard<dxmt::mutex> lock(mutex_);
  name_suballocation_ = suballocation;
}

uint32_t
DynamicBuffer::nextSuballocation() {
  if (name_->hasSuballocatoin(name_suballocation_ + 1)) {
    return ++name_suballocation_;
  }
  return 0;
}

DynamicLinearTexture::DynamicLinearTexture(Texture *texture, Flags<TextureAllocationFlag> flags) :
    texture(texture),
    flags_(flags),
    name_(texture->current()) {}

void
DynamicLinearTexture::incRef() {
  refcount_.fetch_add(1u, std::memory_order_acquire);
};

void
DynamicLinearTexture::decRef() {
  if (refcount_.fetch_sub(1u, std::memory_order_release) == 1u)
    delete this;
};

Rc<TextureAllocation>
DynamicLinearTexture::allocate(uint64_t coherent_seq_id) {
  std::lock_guard<dxmt::mutex> lock(mutex_);
  Rc<TextureAllocation> ret;
  for (;;) {
    if (fifo.empty()) {
      break;
    }
    auto entry = fifo.front();
    if (entry.will_free_at > coherent_seq_id) {
      break;
    }
    ret = std::move(entry.allocation);
    fifo.pop();
    break;
  }
  if (!ret.ptr())
    ret = texture->allocate(flags_);
  return ret;
}

void
DynamicLinearTexture::updateImmediateName(uint64_t current_seq_id, Rc<TextureAllocation> &&allocation, bool owned_by_command_list) {
  std::lock_guard<dxmt::mutex> lock(mutex_);
  if (!owned_by_command_list_)
    fifo.push(QueueEntry{.allocation = std::move(name_), .will_free_at = current_seq_id});
  name_ = std::move(allocation);
  owned_by_command_list_ = owned_by_command_list;
}

void
DynamicLinearTexture::recycle(uint64_t current_seq_id, Rc<TextureAllocation> &&allocation) {
  std::lock_guard<dxmt::mutex> lock(mutex_);
  if (owned_by_command_list_) {
    if (name_.ptr() == allocation.ptr()) {
      owned_by_command_list_ = false;
      auto _ = std::move(allocation);
      return;
    }
  }
  fifo.push(QueueEntry{.allocation = std::move(allocation), .will_free_at = current_seq_id});
}

} // namespace dxmt
