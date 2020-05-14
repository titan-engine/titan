<?php

namespace Titan\Providers;

use Illuminate\Support\Facades\Log;
use Illuminate\Support\ServiceProvider;
use Titan\Commands\MakeExtension;
use Titan\Commands\RefreshExtensionsCache;
use Titan\Commands\InstallTitan;
use Titan\Commands\PublishTitanResources;
use Titan\Commands\SuperAdmin;
use Titan\Commands\UpdateTitan;
use Titan\Extensions;
use Titan\Helpers\Extensions as ExtensionCache;
use Titan\Models\Settings;
use Titan\Observers\StatObserver;

class ExtensionServiceProvider extends ServiceProvider
{

    /**
     * Bootstrap services.
     *
     * @return void
     */
    public function boot()
    {
        $extensions = resolve('extensions')->getCache();

        foreach ($extensions as $ext) {

            if(!$ext['enabled'])
                continue;

            $realNameSpace = $ext['namespace'];
            $folderPath = \Str::kebab($ext['namespace']);
            $folderPath = str_replace(['\\-', '\\'], '/', $folderPath);

            $serviceProvider = "{$realNameSpace}\\ServiceProvider";
            $serviceProviderPath = base_path("{$folderPath}/ServiceProvider.php");

            if (file_exists($serviceProviderPath)) {
                include_once $serviceProviderPath;
                $this->app->register($serviceProvider);
            } else {
                Log::warning("Trying to load {$serviceProvider} {$serviceProviderPath} but failed");
            }
        }
    }

    /**
     * Register services.
     *
     * @return void
     */
    public function register()
    {

        $this->app->singleton('extensions', function () {
            return new ExtensionCache();
        });
    }
}
