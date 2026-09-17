
	/*
	class CommentedExample {
	 public:
	  void CommentedApi();
	};
	*/
	class DBImpl
	    : public DB {
 public:
  struct CompactionStats {
    int64_t bytes_read;
  };

  // Recover the descriptor from persistent storage.  May do a significant
  // amount of work to recover recently logged updates.  Any changes to
  // be made to the descriptor are added to *edit.
  Status Recover(VersionEdit* edit, bool* save_manifest)
      EXCLUSIVE_LOCKS_REQUIRED(mutex_);

  Status RecoverLogFile(uint64_t log_number, bool last_log, bool* save_manifest,
                        VersionEdit* edit, SequenceNumber* max_sequence)
      EXCLUSIVE_LOCKS_REQUIRED(mutex_);

  ~DBImpl();

	#if defined(ENABLE_RECOVERY)
	  Status GuardedRecover(VersionEdit* edit);
	#endif

	  int (*log_filter)(void*);
	  VersionEdit edit_;
	};

			struct Options {
			 public:
			  Status Validate() const;
			  void SetUrl(const char* url = "http://localhost");
			  void SetJson(const char* json = "{}");
			  Status OpenDefault(const Options& opts = default_options());
			  Status ModeDefault(int mode = default_mode);
			  operator bool() const;
			};

			class Compact { public: void Bar(); void Baz(); };
			class CommentedCompact { public: /* doc */ void AfterComment(); };
			class NestedDB { public: struct Iterator { Status Seek(); }; };
			class Q_CORE_EXPORT DB { public: Status Save(); };
			class __attribute__((visibility("default"))) AttributeDB {
			 public:
			  Status Connect();
			};

			LEVELDB_EXPORT class ExportedDB {
			 public:
			  __attribute__((warn_unused_result)) Status Open();
			  __declspec(dllexport) Status Close();
		};

		RK_API struct ExportedOptions { public: Status Load(); };
